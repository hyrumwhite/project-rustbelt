//! Choosing a firing position (`actor_firing_position.c`): an actor's
//! search of its encounter's firing positions for where to fight, take
//! cover, flee, guard or pursue from. The positions of the groups it may use
//! that no nearer actor holds are each scored for how far the actor's path
//! to them is, their range from its target, whether they see it, the dangers
//! and friends' lines of fire near them, the positions it left lately, and
//! (for a pursuit) whether its encounter has looked there; the best one
//! wins. Also the groups its squad allows, and the positions it gave up.
//!
//! The actor's datum, its props and the scenario are the game's memory,
//! worked on in place ([`crate::ai_actor`]); the path searches, the lines of
//! sight, a unit's estimated head or gun, the friends' and enemies' aim, the
//! encounter's owners and pursuit records, the random draw and the change of
//! position are the game's, through [`AiHost::call`]
//! (actor_firing_position.c's `actor_firing_position_call`, shared with the
//! C).

use crate::actor_transitions::{normalize3, point_to_line_distance_squared, vector_to_line_distance_squared};
use crate::ai_actor::{
    AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, PropIterator, prop_get, squad_definition, tag_block_count,
    tag_block_element,
};
use crate::layout::{
    actor_datum as a, actor_definition as d, actor_discarded_firing_position as discarded, actor_variant_definition as var,
    ai_values as v, encounter_definition, firing_position_definition as fpd, prop_datum as p, scenario,
    squad_definition as sq, weapon_definition,
};

/// The selection's calls (halo_sim.h's `_actor_firing_position_call_*`,
/// `(14 << 8) | n`, after the actions').
pub mod call {
    const BASE: i32 = 14 << 8;
    /// The profile's count of searches, and the encounter's positions'
    /// owners (`encounter_build_firing_position_owner_actor_indices`, the
    /// actor's own position left out); in `result2` whether every position is
    /// evaluated (a local game's debug setting).
    pub const OWNERS: i32 = BASE | 1;
    /// The owner of the position `a`, the result; its body's position in
    /// `output[0..3]`.
    pub const OWNER: i32 = BASE | 2;
    /// `unit_estimate_position(unit, a, input[0..3], ...)` (`b`: bit 0 the
    /// facing in `input[3..6]`, bit 1 the gun offset in `input[6..9]`), in
    /// `output[0..3]`.
    pub const ESTIMATE_POSITION: i32 = BASE | 3;
    /// `actor_perception_find_prop_pathfinding_location(actor, a)`.
    pub const FIND_PROP_PATHFINDING_LOCATION: i32 = BASE | 4;
    /// `actor_perception_friend_prop_is_attacking(actor, a, ...)`, the
    /// result; the attack's vector in `output[0..3]`.
    pub const FRIEND_ATTACKING: i32 = BASE | 5;
    /// `unit_get_aiming_vector(a)`, in `output[0..3]`.
    pub const UNIT_AIMING_VECTOR: i32 = BASE | 6;
    /// `actor_nearby_firing_positions(actor, input[0..3], a, b)`, the result.
    pub const NEARBY_FIRING_POSITIONS: i32 = BASE | 7;
    /// `path_3d_available(bsp, input[0..3], input[3], input[4..7])`, the
    /// result.
    pub const PATH_3D_AVAILABLE: i32 = BASE | 8;
    /// The path search from the target's pathfinding point `input[0..3]` on
    /// the surface `a`, twenty units out.
    pub const TARGET_PATH: i32 = BASE | 9;
    /// `path_state_estimated_distance` of the target's search to
    /// `input[0..3]` on the surface `a`: in `output[0]` (and with `b` the
    /// direction in `output[2..5]`).
    pub const TARGET_DISTANCE: i32 = BASE | 10;
    /// The actor's path search (`input[0]` out), with the attractor `a`
    /// (1 the target, 2 the danger zone: its object `b`, point
    /// `input[1..4]`, radius `input[4]`, weight `input[5]`); the result
    /// whether it was found.
    pub const AREA_PATH: i32 = BASE | 11;
    /// `path_state_estimated_distance` of the actor's search to
    /// `input[0..3]` on the surface `a`: in `output[0]`, the closest
    /// approach to the attractor in `output[1]` (and with `b` the direction
    /// in `output[2..5]`).
    pub const AREA_DISTANCE: i32 = BASE | 12;
    /// `seed_random_range(seed, 0, a)`, the result.
    pub const RANDOM_RANGE: i32 = BASE | 13;
    /// `actor_combat_get_firing_variant_definition(actor)`, the result.
    pub const FIRING_VARIANT: i32 = BASE | 14;
    /// `actor_perception_aiming_vector_test_blockage(input[0..3],
    /// input[3..6], input[6..9], NULL)`, the result.
    pub const AIMING_BLOCKAGE: i32 = BASE | 15;
    /// `actor_get_weapon_definition(actor)`, the result.
    pub const WEAPON_DEFINITION: i32 = BASE | 16;
    /// The vehicle `a`'s origin in `output[0..3]`, its forward in
    /// `output[3..6]`; the result whether it moves faster than a twelfth of a
    /// unit a tick.
    pub const VEHICLE: i32 = BASE | 17;
    /// `actor_path_3d_available(actor, input[0..3], ...)`, the result; the
    /// avoidance distance in `output[0]`.
    pub const ACTOR_PATH_3D_AVAILABLE: i32 = BASE | 18;
    /// `encounter_mark_examined_pursuit_position(encounter, actor, a, b)`.
    pub const MARK_EXAMINED: i32 = BASE | 19;
    /// `encounter_pursuit_position_already_examined(encounter, actor, a, b,
    /// ...)`, the result; the last time in `result2`, the examining actors
    /// in `output[0]`.
    pub const ALREADY_EXAMINED: i32 = BASE | 20;
    /// `ai_test_line_of_sight(input[0..3], a & 0xFFFF, input[3..6], a >> 16,
    /// input[6], input[7], b, input[8])`, the result.
    pub const LINE_OF_SIGHT: i32 = BASE | 21;
    /// `actor_change_firing_position(actor, a, NULL, b, <the actor's path
    /// search>, input[0])`, the result.
    pub const CHANGE: i32 = BASE | 22;
    /// The flee's approach to the position `input[3..6]` on the surface `b`
    /// from the prop `a`'s pathfinding point (action_flee.c's path search
    /// to the actor's unit): in `result2` whether the path was found, then
    /// the approach point's result, the point in `output[0..3]` (from
    /// `input[0..3]`).
    pub const FLEE_APPROACH: i32 = BASE | 23;
}

/// actors.h's `_firing_point_evaluation_mode_*`.
pub mod mode {
    pub const FIGHT: i16 = 0;
    pub const PANIC: i16 = 1;
    pub const COVER: i16 = 2;
    pub const UNCOVER: i16 = 3;
    pub const GUARD: i16 = 4;
    pub const PURSUE: i16 = 5;
    pub const AVOID: i16 = 6;
}

/// actors.h's `_firing_position_group_*` (which of a squad's groups).
pub mod group_selection {
    pub const NORMAL: i16 = 0;
    pub const WHEN_SEARCHING: i16 = 1;
    pub const WHEN_NOT_SEARCHING: i16 = 2;
}

const MAXIMUM_FIRING_POSITIONS: usize = 512;
const MAXIMUM_AVOID_POINTS: usize = 32;
const MAXIMUM_ATTACK_VECTORS: usize = 32;
const DISCARDED_FIRING_POSITIONS: u32 = 4;

const ATTACK_VECTOR_FRIEND: i16 = 0;
const ATTACK_VECTOR_FRIEND_PLAYER: i16 = 1;
const ATTACK_VECTOR_DANGEROUS_ENEMY: i16 = 2;

const FLAGS2_AVOID_ALL_ENEMY_ATTACK_VECTORS: u32 = 1 << 0;
const FLAGS2_PATHFINDING_IGNORES_DANGER: u32 = 1 << 4;
const COMBAT_STATUS_DEFINITE: i16 = 3;

const LINE_OF_SIGHT_CLEAR: i16 = v::_AI_LINE_OF_SIGHT_CLEAR as i16;
const LINE_OF_SIGHT_OCCLUDED: i16 = v::_AI_LINE_OF_SIGHT_OCCLUDED as i16;
const LINE_OF_SIGHT_FROM_COVER: i16 = v::_AI_LINE_OF_SIGHT_FROM_COVER as i16;
const LINE_OF_SIGHT_TO_COVER: i16 = 3;
const LINE_OF_SIGHT_OBSTRUCTED: i16 = v::_AI_LINE_OF_SIGHT_OBSTRUCTED as i16;
const LINE_OF_SIGHT_MODE_NORMAL: i16 = 0;
const LINE_OF_SIGHT_MODE_EXPAND_SOURCE: i16 = 1;

const ESTIMATE_HEAD_STANDING: i32 = 1;
const ESTIMATE_HEAD_CROUCHING: i32 = 2;
const ESTIMATE_GUN_POSITION: i32 = 3;

const REAL_EPSILON: f32 = 0.0001;
const TICKS_PER_SECOND: i32 = 30;

/// A position's candidacy (actors.h's `struct firing_position`), with its
/// definition's fields.
#[derive(Debug, Clone, Copy, Default)]
pub struct FiringPosition {
    /// Its `struct firing_position_definition`.
    pub definition: u32,
    pub position: [f32; 3],
    pub group_index: i16,
    pub cluster_index: i16,
    pub surface_index: i32,
    pub original_index: i16,
    pub line_of_sight: i16,
    pub path_distance_from_actor: f32,
    pub path_direction_from_actor: [f32; 3],
    pub path_distance_to_target: f32,
    pub path_closest_approach_to_target: f32,
    pub path_direction_from_target: [f32; 3],
    pub linear_distance_squared_to_target: f32,
    pub valid: bool,
    pub rejected: bool,
    pub pre_evaluation: f32,
    pub evaluation: f32,
}

impl FiringPosition {
    /// The definition's fields.
    fn definition(m: &dyn AiMemory, definition: u32, original_index: i16) -> Self {
        FiringPosition {
            definition,
            position: m.vector(definition + fpd::POSITION as u32),
            group_index: m.i16(definition + fpd::GROUP_INDEX as u32),
            cluster_index: m.i16(definition + fpd::CLUSTER_INDEX as u32),
            surface_index: m.i32(definition + fpd::SURFACE_INDEX as u32),
            original_index,
            ..FiringPosition::default()
        }
    }

    /// A new candidate, as the selection makes them.
    fn candidate(m: &dyn AiMemory, definition: u32, original_index: i16) -> Self {
        FiringPosition {
            line_of_sight: LINE_OF_SIGHT_CLEAR,
            path_distance_from_actor: f32::MAX,
            path_distance_to_target: f32::MAX,
            path_closest_approach_to_target: f32::MAX,
            valid: true,
            ..FiringPosition::definition(m, definition, original_index)
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct AttackVector {
    kind: i16,
    point: [f32; 3],
    vector: [f32; 3],
}

#[derive(Debug, Clone, Copy, Default)]
struct AvoidPoint {
    radius: f32,
    point: [f32; 3],
}

/// What a search asks for, and what it learns on the way (actors.h's
/// `struct firing_position_evaluation_context`; not its debug counts).
#[derive(Debug, Clone, Default)]
pub struct EvaluationContext {
    pub allowed_position_mask: u32,
    pub evaluation_mode: i16,
    /// The union's first byte: a panic's `forced_to_flee`, a cover's
    /// `allow_occluded_points`.
    pub forced_to_flee: bool,
    pub pursue_last_perceived_time: i32,
    pub pursue_tenacious: bool,
    pub allow_rejected_positions: bool,
    pub allow_outside_range: bool,
    pub maximum_allowable_range: f32,
    pub maximum_search_range: f32,
    pub specific_target_enable: bool,
    pub specific_target_point: [f32; 3],
    pub specific_target_surface_index: i32,
    pub specific_target_cluster_index: i16,
    pub attractor_enable: bool,
    pub attractor_weight: f32,
    pub attractor_radius: f32,
    pub find_path_direction_from_actor: bool,
    pub use_last_visible_target_position: bool,
    pub find_path_distance_to_target: bool,
    pub find_path_direction_from_target: bool,
    pub flying: bool,
    pub directional_driving: bool,
    pub directional_driving_cannot_stop: bool,
    pub preferred_groups: u32,
    pub preferred_weight: f32,
    avoid_points: Vec<AvoidPoint>,
    attack_vectors: Vec<AttackVector>,
    friend_attack_vector_count: i16,
    dangerous_enemy_attack_vector_count: i16,
    has_gun_offset_stand: bool,
    gun_offset_stand: [f32; 3],
    has_gun_offset_crouch: bool,
    gun_offset_crouch: [f32; 3],
    pub has_target: bool,
    pub target_current_distance: f32,
    pub target_point: [f32; 3],
    target_head_position: [f32; 3],
    target_line_of_sight_position: [f32; 3],
    target_line_of_sight_optional: bool,
    target_vehicle_index: i32,
    target_pathfinding_surface_index: i32,
    target_pathfinding_point: [f32; 3],
    target_cluster_index: i16,
    target_prop_index: i32,
    target_has_hint_vector: bool,
    target_hint_vector: [f32; 3],
    target_danger_radius: f32,
    post_evaluation_bounded: bool,
    post_evaluation_bound: f32,
}

impl EvaluationContext {
    /// A zeroed search in the mode (the C's `csmemset`).
    pub fn new(evaluation_mode: i16) -> Self {
        EvaluationContext { evaluation_mode, ..EvaluationContext::default() }
    }
}

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[..input.len()].copy_from_slice(input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0, &[]).result
}

fn vector_from_points(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
}

fn magnitude_squared(v: [f32; 3]) -> f32 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    magnitude_squared(vector_from_points(a, b))
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    distance_squared(a, b).sqrt()
}

fn scale(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

/// real_math.h's `point_from_line3d(p, v, t)`.
fn point_from_line(p: [f32; 3], v: [f32; 3], t: f32) -> [f32; 3] {
    [v[0] * t + p[0], v[1] * t + p[1], v[2] * t + p[2]]
}

/// real_math.c's `normalize2d`.
fn normalize2(v: &mut [f32; 2]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if !(REAL_EPSILON > magnitude.abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1]];
        magnitude
    } else {
        0.0
    }
}

/// cseries.h's `MAX(a, b)`.
fn max_c(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// cseries.h's `MIN(a, b)`.
fn min_c(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}

/// cseries.h's `TEST_FLAG(flags, bit)` (a shift past the word wraps, as
/// the x86's does).
fn test_flag(flags: u32, bit: i32) -> bool {
    flags & 1u32.wrapping_shl(bit as u32) != 0
}

fn acknowledged(state: i16) -> bool {
    (v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16..=v::_PROP_STATE_ACKNOWLEDGED as i16).contains(&state)
}

fn orphaned(state: i16) -> bool {
    (v::_PROP_STATE_UNINSPECTED_ORPHAN as i16..=v::_PROP_STATE_INSPECTED_ORPHAN as i16).contains(&state)
}

/// The encounter's definition (the scenario's).
fn encounter_definition(m: &dyn AiMemory, context: &AiStepContext, encounter_index: i32) -> u32 {
    tag_block_element(
        m,
        context.scenario + scenario::AI_ENCOUNTERS as u32,
        encounter_index & 0xFFFF,
        encounter_definition::SIZEOF,
    )
}

/// sort.c's `qsort_4byte`: a quicksort of the indices by `compare(a, b)`
/// (whether `a` goes after `b`), with sort.c's selection sort for eight or
/// fewer.
pub fn qsort_4byte(elements: &mut [i32], mut compare: impl FnMut(i32, i32) -> bool) {
    const STACK_SIZE: usize = 30;
    const CUTOFF: isize = 8;
    if elements.len() < 2 {
        return;
    }
    let mut lo_stack = [0isize; STACK_SIZE];
    let mut hi_stack = [0isize; STACK_SIZE];
    let mut lo: isize = 0;
    let mut hi: isize = elements.len() as isize - 1;
    let mut depth: isize = 0;
    loop {
        let size = hi - lo + 1;
        if size <= CUTOFF {
            // shortsort_4byte
            let mut top = hi;
            while top > lo {
                let mut best = lo;
                let mut scan = lo + 1;
                while scan <= top {
                    if compare(elements[scan as usize], elements[best as usize]) {
                        best = scan;
                    }
                    scan += 1;
                }
                elements.swap(best as usize, top as usize);
                top -= 1;
            }
        } else {
            let mid = lo + (size >> 1);
            elements.swap(mid as usize, lo as usize);
            let mut i = lo;
            let mut j = hi + 1;
            loop {
                loop {
                    i += 1;
                    if !(i <= hi && !compare(elements[i as usize], elements[lo as usize])) {
                        break;
                    }
                }
                loop {
                    j -= 1;
                    if !(j > lo && compare(elements[j as usize], elements[lo as usize])) {
                        break;
                    }
                }
                if j < i {
                    break;
                }
                elements.swap(i as usize, j as usize);
            }
            elements.swap(lo as usize, j as usize);
            // (the C compares byte distances)
            if (j - lo) * 4 - 1 >= (hi - i) * 4 {
                if lo + 1 < j {
                    lo_stack[depth as usize] = lo;
                    hi_stack[depth as usize] = j - 1;
                    depth += 1;
                }
                if i < hi {
                    lo = i;
                    continue;
                }
            } else {
                if i < hi {
                    lo_stack[depth as usize] = i;
                    hi_stack[depth as usize] = hi;
                    depth += 1;
                }
                if lo + 1 < j {
                    hi = j - 1;
                    continue;
                }
            }
        }
        depth -= 1;
        if depth < 0 {
            break;
        }
        lo = lo_stack[depth as usize];
        hi = hi_stack[depth as usize];
    }
}

/// `actor_get_firing_position_group`: the groups of positions the actor's
/// squad allows in the mode (none without an encounter).
pub fn firing_position_group(m: &dyn AiMemory, context: &AiStepContext, evaluation_mode: i16, group_selection_mode: i16) -> u32 {
    let at = |offset: usize| context.actor + offset as u32;
    let encounter_index = m.i32(at(a::META_ENCOUNTER_INDEX));
    if encounter_index == NONE {
        return 0;
    }
    let squad = squad_definition(m, context, encounter_index, m.i16(at(a::META_SQUAD_INDEX)));
    let mut searching = m.bool(at(a::STATE_SEARCHING));
    match group_selection_mode {
        group_selection::WHEN_SEARCHING => searching = true,
        group_selection::WHEN_NOT_SEARCHING => searching = false,
        _ => {}
    }
    let defending = m.bool(at(a::EMOTIONS_CURRENTLY_DEFENDING));
    // ai_scenario_definitions.h's `_firing_position_group_*`
    let index: u32 = if evaluation_mode == mode::PANIC {
        5
    } else if evaluation_mode == mode::GUARD {
        if defending { 5 } else { 2 }
    } else if evaluation_mode == mode::PURSUE {
        6
    } else if defending {
        if searching { 4 } else { 3 }
    } else if searching {
        1
    } else {
        0
    };
    m.r32(squad + sq::FIRING_POSITION_GROUPS as u32 + index * 4)
}

/// `actor_clear_discarded_firing_positions`.
pub fn clear_discarded_firing_positions(m: &dyn AiMemory, context: &AiStepContext, clear_temporary_only: bool) {
    let at = |offset: usize| context.actor + offset as u32;
    m.set_i16(at(a::FIRING_POSITIONS_NEXT_DISCARDED_FIRING_POSITIONS_ENTRY), 0);
    for index in 0..DISCARDED_FIRING_POSITIONS {
        m.set_i16(
            at(a::FIRING_POSITIONS_DISCARDED_FIRING_POSITIONS) + index * discarded::SIZEOF as u32 + discarded::INDEX as u32,
            NONE as i16,
        );
    }
    if m.bool(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION_VALID))
        && (!clear_temporary_only || m.bool(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION_TEMPORARY)))
    {
        m.set_bool(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION_VALID), false);
    }
}

/// `actor_discard_firing_position`.
pub fn discard_firing_position(m: &dyn AiMemory, context: &AiStepContext, firing_position_index: i16, temporary: bool) {
    if firing_position_index == NONE as i16 {
        return;
    }
    let at = |offset: usize| context.actor + offset as u32;
    let cursor_address = at(a::FIRING_POSITIONS_NEXT_DISCARDED_FIRING_POSITIONS_ENTRY);
    let cursor = m.i16(cursor_address);
    let entry = at(a::FIRING_POSITIONS_DISCARDED_FIRING_POSITIONS) + (cursor as i32 as u32).wrapping_mul(discarded::SIZEOF as u32);
    m.set_bool(entry + discarded::TEMPORARY as u32, temporary);
    m.set_i16(entry + discarded::INDEX as u32, firing_position_index);
    m.set_i16(cursor_address, ((cursor as i32 + 1) % DISCARDED_FIRING_POSITIONS as i32) as i16);
    let encounter = encounter_definition(m, context, m.i32(at(a::META_ENCOUNTER_INDEX)));
    let definition = tag_block_element(
        m,
        encounter + encounter_definition::FIRING_POSITIONS as u32,
        firing_position_index as i32,
        fpd::SIZEOF,
    );
    m.set_bool(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION_TEMPORARY), temporary);
    m.set_bool(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION_VALID), true);
    let position = m.vector(definition + fpd::POSITION as u32);
    for (k, value) in position.iter().enumerate() {
        m.set_f32(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION) + 4 * k as u32, *value);
    }
}

/// `actor_firing_position_discarded`.
fn firing_position_discarded(m: &dyn AiMemory, context: &AiStepContext, firing_position_index: i16) -> bool {
    if firing_position_index == NONE as i16 {
        return false;
    }
    (0..DISCARDED_FIRING_POSITIONS).any(|index| {
        m.i16(
            context.actor
                + a::FIRING_POSITIONS_DISCARDED_FIRING_POSITIONS as u32
                + index * discarded::SIZEOF as u32
                + discarded::INDEX as u32,
        ) == firing_position_index
    })
}

/// `firing_position_store_evaluation_debug`.
fn store_evaluation(firing_position: &mut FiringPosition, evaluation: f32) {
    firing_position.evaluation += evaluation;
}

/// `firing_position_reject`: whether the position is out.
fn reject(evaluation_context: &EvaluationContext, firing_position: &mut FiringPosition) -> bool {
    firing_position.rejected = true;
    !evaluation_context.allow_rejected_positions
}

/// `pre_evaluator_global`: the positions given up lately, the danger zone's
/// clearance, the preferred groups, the points to avoid, and a vehicle's
/// heading.
fn pre_evaluator_global(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &EvaluationContext,
    firing_positions: &mut [FiringPosition],
) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    for firing_position in firing_positions.iter_mut() {
        if !firing_position.valid {
            continue;
        }
        if firing_position_discarded(m, context, firing_position.original_index) && reject(evaluation_context, firing_position) {
            firing_position.valid = false;
            continue;
        }
        if evaluation_context.find_path_direction_from_actor {
            let danger_position = m.vector(at(a::DANGER_ZONE_POSITION));
            let danger_vector = vector_from_points(danger_position, m.vector(at(a::DANGER_ZONE_PREDICT_DANGER_POSITION)));
            let center = m.vector(at(a::DANGER_ZONE_BOUNDING_SPHERE_CENTER));
            let radius = m.f32(at(a::DANGER_ZONE_BOUNDING_SPHERE_RADIUS));
            let danger_radius = m.f32(at(a::DANGER_ZONE_DANGER_RADIUS));
            if distance_squared(center, firing_position.position) < (radius + 2.5) * (radius + 2.5) {
                let distance_squared = point_to_line_distance_squared(firing_position.position, danger_position, danger_vector);
                let mut evaluation = 0.0;
                if distance_squared < danger_radius * danger_radius {
                    if reject(evaluation_context, firing_position) {
                        firing_position.valid = false;
                        continue;
                    }
                } else {
                    let clearance_radius = danger_radius + 2.5;
                    if distance_squared < clearance_radius * clearance_radius {
                        evaluation = (distance_squared.sqrt() - danger_radius) * 8.0;
                    } else {
                        evaluation = 20.0;
                    }
                }
                store_evaluation(firing_position, evaluation);
            }
            let body_position = m.vector(at(a::INPUT_BODY_POSITION));
            if distance_squared(center, body_position) < (radius + 3.0) * (radius + 3.0)
                && m.f32(at(a::DANGER_ZONE_CURRENT_DISTANCE_FROM_ACTOR)) > danger_radius
            {
                let distance_squared = point_to_line_distance_squared(body_position, danger_position, danger_vector);
                if distance_squared > danger_radius * danger_radius {
                    let path_vector = scale(firing_position.path_direction_from_actor, 3.0);
                    if magnitude_squared(path_vector) > REAL_EPSILON
                        && vector_to_line_distance_squared(body_position, path_vector, danger_position, danger_vector)
                            < danger_radius * danger_radius
                        && reject(evaluation_context, firing_position)
                    {
                        firing_position.valid = false;
                        continue;
                    }
                }
            }
        }
        if test_flag(evaluation_context.preferred_groups, firing_position.group_index as i32) {
            store_evaluation(firing_position, evaluation_context.preferred_weight);
        }
        if !evaluation_context.avoid_points.is_empty() {
            let mut closest_ratio = 1.0;
            for avoid_point in &evaluation_context.avoid_points {
                let ratio = distance_squared(firing_position.position, avoid_point.point) / (avoid_point.radius * avoid_point.radius);
                if ratio < closest_ratio {
                    closest_ratio = ratio;
                }
            }
            let mut evaluation = 10.0;
            if closest_ratio < 1.0 {
                evaluation = closest_ratio.sqrt() * 10.0;
            }
            store_evaluation(firing_position, evaluation);
        }
    }

    let vehicle_index = m.i32(at(a::INPUT_VEHICLE_INDEX));
    if evaluation_context.directional_driving && vehicle_index != NONE {
        let vehicle = call_full(context, host, call::VEHICLE, vehicle_index, 0, &[]);
        let vehicle_origin = vehicle.vector();
        let forward = [vehicle.output[3], vehicle.output[4], vehicle.output[5]];
        let moving = vehicle.result != 0;
        for firing_position in firing_positions.iter_mut() {
            if !firing_position.valid {
                continue;
            }
            let direction = vector_from_points(vehicle_origin, firing_position.position);
            let distance_squared = magnitude_squared(direction);
            if !((distance_squared as f64).abs() < REAL_EPSILON as f64) && distance_squared < 30.0 * 30.0 {
                let alignment = dot(direction, forward) / distance_squared.sqrt();
                if (evaluation_context.directional_driving_cannot_stop || moving)
                    && distance_squared < 8.0 * 8.0
                    && alignment < 0.70710677
                    && reject(evaluation_context, firing_position)
                {
                    firing_position.valid = false;
                    continue;
                }
                let mut evaluation = 15.0;
                if alignment < 0.0 {
                    evaluation = 15.0 - alignment * -15.0;
                } else if alignment > 0.86602539 {
                    evaluation = (alignment - 0.86602539) * 111.96151 + 15.0;
                }
                store_evaluation(firing_position, evaluation);
            }
        }
    }
}

/// `pre_evaluator_attack`: the range from the target the actor's weapon
/// likes, and dangerous enemies' lines of fire.
fn pre_evaluator_attack(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &EvaluationContext,
    firing_positions: &mut [FiringPosition],
) {
    let m = host.memory();
    let variant_definition = call_simple(context, host, call::FIRING_VARIANT, 0) as u32;
    let variant = |offset: usize| m.f32(variant_definition + offset as u32);
    for firing_position in firing_positions.iter_mut() {
        if !firing_position.valid {
            continue;
        }
        if evaluation_context.has_target {
            let distance_to_target = firing_position.linear_distance_squared_to_target.sqrt();
            if variant(var::RANGED_COMBAT_MAXIMUM_FIRING_RANGE) > 0.0 {
                let preferred_range = variant(var::RANGED_COMBAT_MAXIMUM_FIRING_RANGE) * 0.8;
                let evaluation =
                    if distance_to_target < preferred_range { 10.0 } else { preferred_range / distance_to_target * 10.0 };
                store_evaluation(firing_position, evaluation);
            }
            if variant(var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND) > 0.0
                && distance_to_target < variant(var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND)
            {
                let (mut lower_bound, upper_bound) = if m.bool(context.actor + a::EMOTIONS_BERSERK as u32) {
                    (
                        variant(var::RANGED_COMBAT_BERSERK_FIRING_RANGE_LOWER_BOUND),
                        variant(var::RANGED_COMBAT_BERSERK_FIRING_RANGE_UPPER_BOUND),
                    )
                } else {
                    (variant(var::RANGED_COMBAT_COMBAT_RANGE_LOWER_BOUND), variant(var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND))
                };
                lower_bound = max_c(lower_bound, evaluation_context.target_danger_radius);
                let weapon = call_simple(context, host, call::WEAPON_DEFINITION, 0) as u32;
                if weapon != 0 {
                    let minimum_range = m.f32(weapon + weapon_definition::WEAPON_AI_MINIMUM_TARGET_RANGE as u32);
                    if minimum_range > 0.0 {
                        lower_bound = max_c(lower_bound, minimum_range);
                    }
                }
                let mut distance_from_boundary = upper_bound - distance_to_target;
                if lower_bound > 0.0 {
                    distance_from_boundary = min_c(distance_from_boundary, distance_to_target - lower_bound);
                }
                let mut evaluation = 0.0;
                if distance_from_boundary > 2.0 {
                    evaluation = 20.0;
                } else if distance_from_boundary > 0.0 {
                    evaluation = 20.0 * (distance_from_boundary / 2.0);
                }
                store_evaluation(firing_position, evaluation);
            }
        }
        if evaluation_context.dangerous_enemy_attack_vector_count > 0 {
            let mut nearest_distance_squared = f32::MAX;
            for attack_vector in &evaluation_context.attack_vectors {
                if attack_vector.kind == ATTACK_VECTOR_DANGEROUS_ENEMY {
                    let direction = vector_from_points(attack_vector.point, firing_position.position);
                    let along = dot(direction, attack_vector.vector);
                    if along > 0.0 {
                        let direction = point_from_line(direction, attack_vector.vector, -along);
                        let distance_squared = magnitude_squared(direction);
                        if distance_squared < nearest_distance_squared {
                            nearest_distance_squared = distance_squared;
                        }
                    }
                }
            }
            let mut evaluation = 6.0;
            if nearest_distance_squared < 3.5 * 3.5 {
                evaluation = nearest_distance_squared.sqrt() * 0.25;
            }
            store_evaluation(firing_position, evaluation);
        }
    }
}

/// `pre_evaluator_combatmove`: how near the actor's path makes the
/// position (when its target is in range), and friends' lines of fire.
fn pre_evaluator_combatmove(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &EvaluationContext,
    firing_positions: &mut [FiringPosition],
) {
    let m = host.memory();
    let variant_definition = call_simple(context, host, call::FIRING_VARIANT, 0) as u32;
    let maximum_firing_range = || m.f32(variant_definition + var::RANGED_COMBAT_MAXIMUM_FIRING_RANGE as u32);
    let mut range_weight = 8.0f32;
    let mut in_range = true;
    if evaluation_context.has_target {
        if evaluation_context.target_current_distance > maximum_firing_range() {
            in_range = false;
        } else {
            range_weight = (1.0 - evaluation_context.target_current_distance / maximum_firing_range()) * 8.0;
            in_range = range_weight > 0.0;
        }
    }
    if in_range {
        for firing_position in firing_positions.iter_mut() {
            if firing_position.valid && firing_position.path_distance_from_actor < evaluation_context.maximum_allowable_range {
                store_evaluation(
                    firing_position,
                    max_c(0.0, 1.0 - firing_position.path_distance_from_actor / evaluation_context.maximum_allowable_range)
                        * range_weight,
                );
            }
        }
    }
    if evaluation_context.friend_attack_vector_count > 0 {
        for firing_position in firing_positions.iter_mut() {
            if !firing_position.valid {
                continue;
            }
            let mut worst_friend_blockage: i16 = 0;
            let mut worst_player_blockage: i16 = 0;
            for attack_vector in &evaluation_context.attack_vectors {
                if attack_vector.kind == ATTACK_VECTOR_FRIEND || attack_vector.kind == ATTACK_VECTOR_FRIEND_PLAYER {
                    let mut input = [0.0f32; 9];
                    input[0..3].copy_from_slice(&attack_vector.point);
                    input[3..6].copy_from_slice(&attack_vector.vector);
                    input[6..9].copy_from_slice(&firing_position.position);
                    let blockage = call_full(context, host, call::AIMING_BLOCKAGE, 0, 0, &input).result as i16;
                    if attack_vector.kind == ATTACK_VECTOR_FRIEND {
                        worst_friend_blockage = worst_friend_blockage.max(blockage);
                    } else if attack_vector.kind == ATTACK_VECTOR_FRIEND_PLAYER {
                        worst_player_blockage = worst_player_blockage.max(blockage);
                    }
                }
            }
            let evaluation = if worst_player_blockage >= 2 {
                0.0
            } else if worst_player_blockage >= 1 {
                1.5
            } else if worst_friend_blockage >= 2 {
                6.0
            } else if worst_friend_blockage >= 1 {
                8.5
            } else {
                10.0
            };
            firing_position.evaluation += evaluation;
        }
    }
}

/// `pre_evaluator_guard`: how near the actor's path makes the position.
fn pre_evaluator_guard(evaluation_context: &EvaluationContext, firing_positions: &mut [FiringPosition]) {
    for firing_position in firing_positions.iter_mut() {
        if firing_position.valid {
            let half_range = evaluation_context.maximum_allowable_range * 0.5;
            let mut evaluation = 0.0;
            if firing_position.path_distance_from_actor < half_range {
                evaluation = 8.0;
            } else if firing_position.path_distance_from_actor < evaluation_context.maximum_allowable_range {
                evaluation = (1.0 / half_range)
                    * (evaluation_context.maximum_allowable_range - firing_position.path_distance_from_actor)
                    * 8.0;
            }
            store_evaluation(firing_position, evaluation);
        }
    }
}

/// `pre_evaluator_panic`: away from the actor, and from its target and the
/// target's path.
fn pre_evaluator_panic(evaluation_context: &EvaluationContext, firing_positions: &mut [FiringPosition]) {
    for firing_position in firing_positions.iter_mut() {
        if !firing_position.valid {
            continue;
        }
        if firing_position.path_distance_from_actor < 4.0 {
            if reject(evaluation_context, firing_position) {
                firing_position.valid = false;
                continue;
            }
        } else {
            let mut evaluation = 0.0;
            if firing_position.path_distance_from_actor < 8.0 {
                evaluation = (firing_position.path_distance_from_actor - 4.0) * 2.0;
            } else if firing_position.path_distance_from_actor < evaluation_context.maximum_allowable_range {
                evaluation = (evaluation_context.maximum_allowable_range - firing_position.path_distance_from_actor) * 8.0
                    / (evaluation_context.maximum_allowable_range - 8.0);
            }
            store_evaluation(firing_position, evaluation);
        }
        if evaluation_context.has_target {
            if firing_position.linear_distance_squared_to_target < 16.0 && reject(evaluation_context, firing_position) {
                firing_position.valid = false;
                continue;
            }
            let evaluation = if firing_position.linear_distance_squared_to_target < 16.0 {
                0.0
            } else if firing_position.linear_distance_squared_to_target < 49.0 {
                (firing_position.linear_distance_squared_to_target.sqrt() - 4.0) * (10.0f32 / 3.0)
            } else {
                10.0
            };
            store_evaluation(firing_position, evaluation);
            if firing_position.path_closest_approach_to_target < f32::MAX
                && evaluation_context.target_current_distance > 0.0
                && evaluation_context.target_current_distance < f32::MAX
            {
                let proximity = 1.0
                    - firing_position.path_closest_approach_to_target / (evaluation_context.target_current_distance * 0.8);
                if proximity > 0.5 && reject(evaluation_context, firing_position) {
                    firing_position.valid = false;
                    continue;
                }
                let pinned = if proximity < 0.0 {
                    0.0
                } else if proximity > 1.0 {
                    1.0
                } else {
                    proximity
                };
                store_evaluation(firing_position, (1.0 - pinned) * 8.0);
            }
        }
    }
}

/// `pre_evaluator_pursuit`: near the actor's path, near the target's, and
/// along the orphan's hint.
fn pre_evaluator_pursuit(evaluation_context: &EvaluationContext, firing_positions: &mut [FiringPosition]) {
    for firing_position in firing_positions.iter_mut() {
        if !firing_position.valid {
            continue;
        }
        let half_range = evaluation_context.maximum_allowable_range * 0.5;
        let mut evaluation = 0.0;
        if firing_position.path_distance_from_actor < half_range {
            evaluation = 5.0;
        } else if firing_position.path_distance_from_actor < evaluation_context.maximum_allowable_range {
            evaluation = (1.0 / half_range)
                * (evaluation_context.maximum_allowable_range - firing_position.path_distance_from_actor)
                * 5.0;
        }
        store_evaluation(firing_position, evaluation);
        if evaluation_context.has_target {
            if firing_position.path_distance_to_target < 20.0 {
                store_evaluation(firing_position, (20.0 - firing_position.path_distance_to_target) * 0.5);
            }
            if evaluation_context.find_path_direction_from_target && evaluation_context.target_has_hint_vector {
                let alignment = dot(firing_position.path_direction_from_target, evaluation_context.target_hint_vector);
                let evaluation =
                    if alignment > 0.70710677 { 10.0 } else { max_c(0.0, alignment * 1.4142135) * 10.0 };
                store_evaluation(firing_position, evaluation);
            }
        }
    }
}

/// `firing_position_pre_evaluate`: global_pre_evaluator_table's evaluators
/// of the mode.
fn pre_evaluate(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &EvaluationContext,
    firing_positions: &mut [FiringPosition],
) {
    let mode = evaluation_context.evaluation_mode;
    let flag = |mask: i32| (mask >> mode) & 1 != 0;
    pre_evaluator_global(context, host, evaluation_context, firing_positions);
    if flag(1 << mode::FIGHT | 1 << mode::UNCOVER) {
        pre_evaluator_attack(context, host, evaluation_context, firing_positions);
    }
    if flag(1 << mode::FIGHT | 1 << mode::COVER | 1 << mode::UNCOVER | 1 << mode::AVOID) {
        pre_evaluator_combatmove(context, host, evaluation_context, firing_positions);
    }
    if flag(1 << mode::GUARD) {
        pre_evaluator_guard(evaluation_context, firing_positions);
    }
    if flag(1 << mode::PANIC) {
        pre_evaluator_panic(evaluation_context, firing_positions);
    }
    if flag(1 << mode::PURSUE) {
        pre_evaluator_pursuit(evaluation_context, firing_positions);
    }
}

/// `post_evaluator_global`: a flyer's path there (with none, the bound).
fn post_evaluator_global(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &mut EvaluationContext,
    firing_position: Option<&mut FiringPosition>,
) -> bool {
    let Some(firing_position) = firing_position else {
        if evaluation_context.flying {
            evaluation_context.post_evaluation_bound += 15.0;
        }
        return true;
    };
    if evaluation_context.flying {
        let available = call_full(context, host, call::ACTOR_PATH_3D_AVAILABLE, 0, 0, &firing_position.position);
        let mut path_available = available.result != 0;
        if path_available {
            let body_position = host.memory().vector(context.actor + a::INPUT_BODY_POSITION as u32);
            let mut input = [0.0f32; 7];
            input[0..3].copy_from_slice(&body_position);
            input[3] = available.output[0];
            input[4..7].copy_from_slice(&firing_position.position);
            path_available = call_full(context, host, call::PATH_3D_AVAILABLE, 0, 0, &input).result != 0;
        }
        if path_available {
            store_evaluation(firing_position, 15.0);
        } else if reject(evaluation_context, firing_position) {
            firing_position.valid = false;
        }
    }
    firing_position.valid
}

/// `post_evaluator_pursuit`: whether the encounter has looked there, how
/// lately and by how many.
fn post_evaluator_pursuit(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &mut EvaluationContext,
    firing_position: Option<&mut FiringPosition>,
) -> bool {
    let current_time = context.game_time;
    let Some(firing_position) = firing_position else {
        return false;
    };
    let last_examined_time;
    let examining_actor_count: i16;
    let mut fresh = true;
    if firing_position.line_of_sight == LINE_OF_SIGHT_CLEAR && firing_position.path_distance_from_actor < 6.0 {
        crate::pursuits::mark_examined(
            host.memory(),
            context,
            firing_position.original_index,
            evaluation_context.pursue_last_perceived_time,
        );
        last_examined_time = current_time;
        examining_actor_count = 7;
        fresh = false;
    } else {
        let (examined, count, time) = crate::pursuits::already_examined(
            host.memory(),
            context,
            firing_position.original_index,
            evaluation_context.pursue_last_perceived_time,
        );
        last_examined_time = time;
        examining_actor_count = count;
        if examined {
            fresh = false;
        }
    }
    if evaluation_context.pursue_tenacious {
        if fresh {
            store_evaluation(firing_position, 15.0);
        }
    } else if !fresh && reject(evaluation_context, firing_position) {
        firing_position.valid = false;
    }
    if firing_position.valid {
        let mut evaluation = 0.0;
        if last_examined_time == NONE || last_examined_time + 10 * TICKS_PER_SECOND < current_time {
            evaluation = 10.0;
        } else if last_examined_time < current_time {
            evaluation = (current_time - last_examined_time) as f32 * (1.0f32 / TICKS_PER_SECOND as f32);
        }
        store_evaluation(firing_position, evaluation);
        let mut evaluation = 0.0;
        if examining_actor_count < 4 {
            evaluation = (4 - examining_actor_count) as f32 * 5.0;
        }
        store_evaluation(firing_position, evaluation);
    }
    firing_position.valid
}

/// `post_evaluator_hide`: hidden from the target.
fn post_evaluator_hide(evaluation_context: &mut EvaluationContext, firing_position: Option<&mut FiringPosition>) -> bool {
    let Some(firing_position) = firing_position else {
        if evaluation_context.has_target {
            evaluation_context.post_evaluation_bound += 12.0;
        }
        return true;
    };
    if evaluation_context.has_target {
        let mut evaluation = 0.0;
        match firing_position.line_of_sight {
            LINE_OF_SIGHT_FROM_COVER => evaluation = 12.0,
            LINE_OF_SIGHT_OBSTRUCTED => evaluation = 10.0,
            LINE_OF_SIGHT_OCCLUDED => {
                if evaluation_context.forced_to_flee {
                    evaluation = 6.0;
                } else if reject(evaluation_context, firing_position) {
                    firing_position.valid = false;
                }
            }
            LINE_OF_SIGHT_TO_COVER => evaluation = 4.0,
            LINE_OF_SIGHT_CLEAR => {
                if reject(evaluation_context, firing_position) {
                    firing_position.valid = false;
                }
            }
            _ => {}
        }
        store_evaluation(firing_position, evaluation);
    }
    firing_position.valid
}

/// `post_evaluator_uncover`: seeing the target, or nearer it.
fn post_evaluator_uncover(evaluation_context: &mut EvaluationContext, firing_position: Option<&mut FiringPosition>) -> bool {
    let Some(firing_position) = firing_position else {
        if evaluation_context.has_target {
            evaluation_context.post_evaluation_bound += 20.0;
        }
        return true;
    };
    if evaluation_context.has_target {
        let mut evaluation = 0.0;
        match firing_position.line_of_sight {
            LINE_OF_SIGHT_CLEAR => evaluation = 20.0,
            LINE_OF_SIGHT_OCCLUDED => evaluation = 10.0,
            _ => {
                let closer_distance = evaluation_context.target_current_distance - 7.5;
                if (closer_distance < 0.0
                    || firing_position.linear_distance_squared_to_target > closer_distance * closer_distance)
                    && reject(evaluation_context, firing_position)
                {
                    firing_position.valid = false;
                }
            }
        }
        store_evaluation(firing_position, evaluation);
    }
    firing_position.valid
}

/// `post_evaluator_attack`: seeing the target.
fn post_evaluator_attack(evaluation_context: &mut EvaluationContext, firing_position: Option<&mut FiringPosition>) -> bool {
    let optional = evaluation_context.target_line_of_sight_optional;
    let Some(firing_position) = firing_position else {
        if evaluation_context.has_target {
            evaluation_context.post_evaluation_bound += if optional { 6.0 } else { 15.0 };
        }
        return true;
    };
    if evaluation_context.has_target {
        let mut evaluation = 0.0;
        match firing_position.line_of_sight {
            LINE_OF_SIGHT_CLEAR => evaluation = if optional { 6.0 } else { 15.0 },
            LINE_OF_SIGHT_OCCLUDED => evaluation = if optional { 2.5 } else { 5.0 },
            _ => {
                if !optional && reject(evaluation_context, firing_position) {
                    firing_position.valid = false;
                }
            }
        }
        store_evaluation(firing_position, evaluation);
    }
    firing_position.valid
}

/// `firing_position_post_evaluate` (with no position,
/// `firing_positions_get_post_evaluation_bound`'s walk):
/// global_post_evaluator_table's evaluators of the mode, until one finds the
/// position out.
fn post_evaluate(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &mut EvaluationContext,
    mut firing_position: Option<&mut FiringPosition>,
) -> bool {
    let mode = evaluation_context.evaluation_mode;
    let flag = |mask: i32| (mask >> mode) & 1 != 0;
    let mut valid = post_evaluator_global(context, host, evaluation_context, firing_position.as_deref_mut());
    if valid && flag(1 << mode::FIGHT | 1 << mode::GUARD | 1 << mode::AVOID) {
        valid = post_evaluator_attack(evaluation_context, firing_position.as_deref_mut());
    }
    if valid && flag(1 << mode::UNCOVER) {
        valid = post_evaluator_uncover(evaluation_context, firing_position.as_deref_mut());
    }
    if valid && flag(1 << mode::PANIC | 1 << mode::COVER) {
        valid = post_evaluator_hide(evaluation_context, firing_position.as_deref_mut());
    }
    if valid && flag(1 << mode::PURSUE) {
        valid = post_evaluator_pursuit(context, host, evaluation_context, firing_position.as_deref_mut());
    }
    valid
}

/// `firing_positions_get_post_evaluation_bound`: the most the post
/// evaluators can add, and whether that bounds the search.
fn post_evaluation_bound(context: &AiStepContext, host: &impl AiHost, evaluation_context: &mut EvaluationContext) -> bool {
    evaluation_context.post_evaluation_bound = 0.0;
    post_evaluate(context, host, evaluation_context, None)
}

/// `firing_position_compare`: whether the first goes after the second
/// (the valid, then the unrejected, then the best first).
fn compare(first: &FiringPosition, second: &FiringPosition) -> bool {
    if first.valid != second.valid {
        return !first.valid;
    }
    if first.rejected != second.rejected {
        return first.rejected;
    }
    if first.evaluation > second.evaluation {
        return false;
    }
    first.evaluation < second.evaluation
}

/// `unit_estimate_position` of the actor's unit at the point.
fn estimate_position(
    context: &AiStepContext,
    host: &impl AiHost,
    estimate_mode: i32,
    point: [f32; 3],
    facing: Option<[f32; 3]>,
    gun_offset: Option<[f32; 3]>,
) -> [f32; 3] {
    let mut input = [0.0f32; 9];
    input[0..3].copy_from_slice(&point);
    if let Some(facing) = facing {
        input[3..6].copy_from_slice(&facing);
    }
    if let Some(gun_offset) = gun_offset {
        input[6..9].copy_from_slice(&gun_offset);
    }
    let flags = facing.is_some() as i32 | (gun_offset.is_some() as i32) << 1;
    call_full(context, host, call::ESTIMATE_POSITION, estimate_mode, flags, &input).vector()
}

/// `ai_test_line_of_sight` ([`crate::sensing`]).
#[allow(clippy::too_many_arguments)]
fn test_line_of_sight(
    context: &AiStepContext,
    host: &impl AiHost,
    point0: [f32; 3],
    cluster0: i16,
    point1: [f32; 3],
    cluster1: i16,
    line_of_sight_mode: i16,
    test_line_of_fire: bool,
    ignore_object_index: i32,
    ignore_vehicles: bool,
) -> i16 {
    crate::sensing::ai_test_line_of_sight(
        context,
        host,
        point0,
        cluster0,
        point1,
        cluster1,
        line_of_sight_mode,
        test_line_of_fire,
        ignore_object_index,
        ignore_vehicles,
    )
}

/// `firing_position_compute_line_of_sight`: whether the actor would see
/// its target from the position (in a pursuit, whether it sees the position).
fn compute_line_of_sight(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &EvaluationContext,
    firing_position: &mut FiringPosition,
) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let in_vehicle = m.i32(at(a::INPUT_VEHICLE_INDEX)) != NONE;
    if evaluation_context.evaluation_mode == mode::PURSUE {
        if firing_position.path_distance_from_actor < 6.0 {
            let estimated_position =
                estimate_position(context, host, ESTIMATE_HEAD_STANDING, firing_position.position, None, None);
            firing_position.line_of_sight = test_line_of_sight(
                context,
                host,
                m.vector(at(a::INPUT_POSITION_HEAD_POSITION)),
                m.i16(at(a::INPUT_POSITION_BODY_LOCATION_CLUSTER_INDEX)),
                estimated_position,
                firing_position.cluster_index,
                LINE_OF_SIGHT_MODE_NORMAL,
                false,
                NONE,
                in_vehicle,
            );
        } else {
            firing_position.line_of_sight = LINE_OF_SIGHT_OBSTRUCTED;
        }
        return;
    }
    let mode = evaluation_context.evaluation_mode;
    let (estimate_mode, facing, gun_offset) = if mode == mode::PANIC || mode == mode::COVER {
        (ESTIMATE_HEAD_CROUCHING, None, None)
    } else if evaluation_context.has_gun_offset_stand {
        let mut facing = vector_from_points(firing_position.position, evaluation_context.target_head_position);
        let mut facing2 = [facing[0], facing[1]];
        let desired_facing = if normalize2(&mut facing2) > 0.0 {
            facing = [facing2[0], facing2[1], 0.0];
            facing
        } else {
            m.vector(at(a::INPUT_FACING_VECTOR))
        };
        (ESTIMATE_GUN_POSITION, Some(desired_facing), Some(evaluation_context.gun_offset_stand))
    } else {
        (ESTIMATE_HEAD_STANDING, None, None)
    };
    let estimated_position = estimate_position(context, host, estimate_mode, firing_position.position, facing, gun_offset);
    let line_of_sight_mode = if mode > mode::FIGHT && mode <= mode::UNCOVER {
        LINE_OF_SIGHT_MODE_EXPAND_SOURCE
    } else {
        LINE_OF_SIGHT_MODE_NORMAL
    };
    firing_position.line_of_sight = test_line_of_sight(
        context,
        host,
        estimated_position,
        firing_position.cluster_index,
        evaluation_context.target_line_of_sight_position,
        evaluation_context.target_cluster_index,
        line_of_sight_mode,
        true,
        evaluation_context.target_vehicle_index,
        in_vehicle,
    );
}

/// `firing_position_forced_evaluation`: the one position evaluated alone.
fn forced_evaluation(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &mut EvaluationContext,
    firing_position: &mut FiringPosition,
) -> bool {
    firing_position.valid = true;
    firing_position.evaluation = 0.0;
    firing_position.pre_evaluation = 0.0;
    firing_position.rejected = false;
    pre_evaluate(context, host, evaluation_context, std::slice::from_mut(firing_position));
    if firing_position.valid {
        if evaluation_context.has_target {
            compute_line_of_sight(context, host, evaluation_context, firing_position);
        }
        firing_position.pre_evaluation = firing_position.evaluation;
        firing_position.valid = post_evaluate(context, host, evaluation_context, Some(firing_position));
    }
    firing_position.valid
}

/// `actor_select_firing_position`: the best of the encounter's positions
/// the search allows (its index, or NONE), with its candidacy, its owner
/// and whether the actor's path search was found (the game keeps it, for
/// the change).
pub fn select_firing_position(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &mut EvaluationContext,
    best_firing_position: &mut FiringPosition,
    current_owner: &mut i32,
    area_path_state_valid: &mut bool,
) -> i16 {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut best_index = NONE as i16;
    let mut best_evaluation = 0.0f32;
    let encounter_index = m.i32(at(a::META_ENCOUNTER_INDEX));
    if encounter_index == NONE {
        return best_index;
    }
    let definition = |offset: usize| context.definition + offset as u32;
    let encounter = encounter_definition(m, context, encounter_index);
    let evaluation_mode = evaluation_context.evaluation_mode;
    let evaluate_all = call_simple_full(context, host, call::OWNERS, 0).result2 != 0;

    evaluation_context.maximum_allowable_range =
        if m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) == v::_ACTOR_VEHICLE_DRIVER_NONE as i16 { 15.0 } else { 80.0 };
    if evaluation_context.maximum_search_range == 0.0 {
        evaluation_context.maximum_search_range = evaluation_context.maximum_allowable_range;
    }
    evaluation_context.find_path_distance_to_target = evaluation_mode == mode::PURSUE;
    evaluation_context.has_target = false;

    let body_position = m.vector(at(a::INPUT_BODY_POSITION));
    if evaluation_context.specific_target_enable {
        evaluation_context.target_point = evaluation_context.specific_target_point;
        evaluation_context.has_target = true;
        evaluation_context.target_pathfinding_point = evaluation_context.specific_target_point;
        evaluation_context.target_pathfinding_surface_index = evaluation_context.specific_target_surface_index;
        evaluation_context.target_cluster_index = evaluation_context.specific_target_cluster_index;
        evaluation_context.target_current_distance = distance(body_position, evaluation_context.target_point);
        evaluation_context.target_prop_index = NONE;
        evaluation_context.target_vehicle_index = NONE;
        evaluation_context.target_danger_radius = 0.0;
        evaluation_context.target_head_position =
            estimate_position(context, host, ESTIMATE_HEAD_STANDING, evaluation_context.target_point, None, None);
        evaluation_context.target_line_of_sight_position = evaluation_context.target_head_position;
    } else {
        let mut target_prop_index = NONE;
        if m.i16(at(a::STATE_ACTION)) == v::_ACTOR_ACTION_FLEE as i16 && m.i32(at(a::STATE_FLEE_FLEE_PROP_INDEX)) != NONE {
            target_prop_index = m.i32(at(a::STATE_FLEE_FLEE_PROP_INDEX));
        } else if m.i32(at(a::TARGET_TARGET_PROP_INDEX)) != NONE {
            target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
        }
        if target_prop_index != NONE {
            let prop = prop_get(m, context, target_prop_index);
            if evaluation_context.find_path_distance_to_target && acknowledged(m.i16(prop + p::STATE as u32)) {
                call_simple(context, host, call::FIND_PROP_PATHFINDING_LOCATION, target_prop_index);
            }
            let field = |offset: usize| prop + offset as u32;
            evaluation_context.has_target = true;
            evaluation_context.target_point = m.vector(field(p::BODY_POSITION));
            evaluation_context.target_pathfinding_point = m.vector(field(p::PATHFINDING_POINT));
            evaluation_context.target_pathfinding_surface_index = m.i32(field(p::PATHFINDING_SURFACE_INDEX));
            evaluation_context.target_cluster_index = m.i16(field(p::BODY_LOCATION_CLUSTER_INDEX));
            evaluation_context.target_current_distance = m.f32(field(p::DISTANCE));
            evaluation_context.target_prop_index = target_prop_index;
            evaluation_context.target_head_position = m.vector(field(p::HEAD_POSITION));
            evaluation_context.target_vehicle_index = m.i32(field(p::VEHICLE_INDEX));
            evaluation_context.target_danger_radius = m.f32(field(p::SUICIDE_RADIUS));
            evaluation_context.target_line_of_sight_position =
                if evaluation_context.use_last_visible_target_position && m.i32(field(p::LAST_VISIBLE_TIME)) != NONE {
                    m.vector(field(p::LAST_VISIBLE_HEAD_POSITION))
                } else {
                    m.vector(field(p::HEAD_POSITION))
                };
            if orphaned(m.i16(field(p::STATE))) {
                evaluation_context.target_has_hint_vector = true;
                evaluation_context.target_hint_vector = m.vector(field(p::ORPHAN_HINT_VECTOR));
            }
            evaluation_context.target_line_of_sight_optional = evaluation_mode == mode::GUARD || evaluation_mode == mode::AVOID;
        }
    }

    let variant = |offset: usize| m.vector(context.variant_definition + offset as u32);
    let gun_offset = |variant_offset: [f32; 3], definition_offset: [f32; 3]| {
        if magnitude_squared(variant_offset) > 0.0001 {
            Some(variant_offset)
        } else if magnitude_squared(definition_offset) > 0.0001 {
            Some(definition_offset)
        } else {
            None
        }
    };
    match gun_offset(variant(var::RANGED_COMBAT_GUN_OFFSET_STAND), m.vector(definition(d::PERCEPTION_GUN_OFFSET_STAND))) {
        Some(offset) => {
            evaluation_context.has_gun_offset_stand = true;
            evaluation_context.gun_offset_stand = offset;
        }
        None => evaluation_context.has_gun_offset_stand = false,
    }
    match gun_offset(variant(var::RANGED_COMBAT_GUN_OFFSET_CROUCH), m.vector(definition(d::PERCEPTION_GUN_OFFSET_CROUCH))) {
        Some(offset) => {
            evaluation_context.has_gun_offset_crouch = true;
            evaluation_context.gun_offset_crouch = offset;
        }
        None => evaluation_context.has_gun_offset_crouch = false,
    }

    let danger_type = m.i16(at(a::DANGER_ZONE_DANGER_TYPE));
    if danger_type > v::_ACTOR_DANGER_ZONE_NONE as i16
        && m.bool(at(a::DANGER_ZONE_NOTICED_DANGER))
        && m.f32(at(a::DANGER_ZONE_CURRENT_DISTANCE_FROM_ACTOR)) < m.f32(at(a::DANGER_ZONE_BOUNDING_SPHERE_RADIUS)) + 3.0
    {
        evaluation_context.find_path_direction_from_actor = true;
    }
    evaluation_context.flying = m.bool(at(a::STATE_FLYING));
    if m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16 {
        evaluation_context.directional_driving = true;
        evaluation_context.directional_driving_cannot_stop = true;
    }

    evaluation_context.avoid_points.clear();
    if m.bool(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION_VALID)) {
        evaluation_context.avoid_points.push(AvoidPoint {
            radius: m.f32(definition(d::FIRING_POSITION_OLD_AVOIDANCE_RADIUS)),
            point: m.vector(at(a::FIRING_POSITIONS_LAST_DISCARDED_FIRING_POSITION)),
        });
    }
    let friend_avoidance_radius = m.f32(definition(d::FIRING_POSITION_FRIEND_AVOIDANCE_RADIUS));
    if friend_avoidance_radius > 0.0 && matches!(evaluation_mode, mode::FIGHT | mode::UNCOVER | mode::AVOID) {
        let mut iterator = PropIterator::new(m, context);
        while evaluation_context.avoid_points.len() < MAXIMUM_AVOID_POINTS {
            let Some(prop) = iterator.next(m, context) else { break };
            let field = |offset: usize| prop + offset as u32;
            if acknowledged(m.i16(field(p::STATE))) && !m.bool(field(p::ENEMY)) && !m.bool(field(p::DEAD)) && !m.bool(field(p::PLAYER))
            {
                evaluation_context
                    .avoid_points
                    .push(AvoidPoint { radius: friend_avoidance_radius, point: m.vector(field(p::BODY_POSITION)) });
            }
        }
    }

    evaluation_context.attack_vectors.clear();
    evaluation_context.friend_attack_vector_count = 0;
    evaluation_context.dangerous_enemy_attack_vector_count = 0;
    {
        let flags = m.r32(definition(d::FLAGS));
        let flags2 = m.r32(definition(d::FLAGS2));
        let combat_status = m.i16(at(a::STATE_COMBAT_STATUS));
        let mut build_attack_vectors = false;
        if test_flag(flags, v::_ACTOR_DEFINITION_AVOID_FRIEND_LINE_OF_FIRE_BIT as i32)
            && combat_status >= COMBAT_STATUS_DEFINITE
            && m.i8(at(a::SITUATION_AREA_FRIENDS)) > 0
        {
            build_attack_vectors = true;
        }
        if flags2 & FLAGS2_AVOID_ALL_ENEMY_ATTACK_VECTORS != 0 && combat_status >= COMBAT_STATUS_DEFINITE {
            build_attack_vectors = true;
        }
        if build_attack_vectors && matches!(evaluation_mode, mode::FIGHT | mode::COVER | mode::UNCOVER | mode::AVOID) {
            let mut iterator = PropIterator::new(m, context);
            while evaluation_context.attack_vectors.len() < MAXIMUM_ATTACK_VECTORS {
                let Some(prop) = iterator.next(m, context) else { break };
                let field = |offset: usize| prop + offset as u32;
                if !acknowledged(m.i16(field(p::STATE))) || m.bool(field(p::DEAD)) {
                    continue;
                }
                let enemy = m.bool(field(p::ENEMY));
                let player = m.bool(field(p::PLAYER));
                let vehicle_index = m.i32(field(p::VEHICLE_INDEX));
                if !enemy && (player || vehicle_index == NONE) {
                    let attacking = call_simple_full(context, host, call::FRIEND_ATTACKING, iterator.index);
                    if attacking.result != 0 {
                        evaluation_context.attack_vectors.push(AttackVector {
                            kind: if player { ATTACK_VECTOR_FRIEND_PLAYER } else { ATTACK_VECTOR_FRIEND },
                            point: m.vector(field(p::BODY_POSITION)),
                            vector: attacking.vector(),
                        });
                        evaluation_context.friend_attack_vector_count += 1;
                    }
                }
                if enemy {
                    let mut dangerous = flags2 & FLAGS2_AVOID_ALL_ENEMY_ATTACK_VECTORS != 0;
                    if m.bool(field(p::UNOPPOSABLE_ENEMY)) && (player || m.bool(field(p::SHOOTING))) {
                        dangerous = true;
                    }
                    if dangerous {
                        let unit_index = if vehicle_index != NONE { vehicle_index } else { m.i32(field(p::UNIT_INDEX)) };
                        let vector = call_simple_full(context, host, call::UNIT_AIMING_VECTOR, unit_index).vector();
                        evaluation_context.attack_vectors.push(AttackVector {
                            kind: ATTACK_VECTOR_DANGEROUS_ENEMY,
                            point: m.vector(field(p::BODY_POSITION)),
                            vector,
                        });
                        evaluation_context.dangerous_enemy_attack_vector_count += 1;
                    }
                }
            }
        }
    }

    *area_path_state_valid = false;
    let mut firing_positions: Vec<FiringPosition> = Vec::new();
    let firing_position_block = encounter + encounter_definition::FIRING_POSITIONS as u32;
    let firing_position_count = tag_block_count(m, firing_position_block);
    let mut index: i32 = 0;
    while index < firing_position_count && index < MAXIMUM_FIRING_POSITIONS as i32 {
        let firing_position_definition = tag_block_element(m, firing_position_block, index, fpd::SIZEOF);
        let candidate = FiringPosition::candidate(m, firing_position_definition, index as i16);
        if test_flag(evaluation_context.allowed_position_mask, candidate.group_index as i32)
            && (evaluation_context.flying || candidate.surface_index != NONE)
            && (evaluation_mode != mode::PURSUE
                || call_full(
                    context,
                    host,
                    call::NEARBY_FIRING_POSITIONS,
                    candidate.surface_index,
                    group_selection::WHEN_SEARCHING as i32,
                    &candidate.position,
                )
                .result
                    != 0)
        {
            let mut acceptable = true;
            let owner = call_simple_full(context, host, call::OWNER, index);
            if owner.result != NONE {
                if evaluation_mode == mode::GUARD && evaluation_context.avoid_points.len() < MAXIMUM_AVOID_POINTS {
                    evaluation_context.avoid_points.push(AvoidPoint { radius: 4.0, point: candidate.position });
                }
                let owner_distance = distance(owner.vector(), candidate.position);
                let actor_distance = distance(body_position, candidate.position);
                if owner_distance < 1.0 || owner_distance < 2.0 * actor_distance {
                    acceptable = false;
                }
            }
            if acceptable {
                firing_positions.push(candidate);
            }
        }
        index += 1;
    }

    if firing_positions.is_empty() {
        clear_discarded_firing_positions(m, context, false);
        return best_index;
    }

    if evaluation_context.has_target && evaluation_context.find_path_distance_to_target {
        if evaluation_context.flying {
            for firing_position in firing_positions.iter_mut() {
                let mut target_vector = vector_from_points(evaluation_context.target_point, firing_position.position);
                if magnitude_squared(target_vector) < 400.0 {
                    let mut input = [0.0f32; 7];
                    input[0..3].copy_from_slice(&evaluation_context.target_point);
                    input[4..7].copy_from_slice(&firing_position.position);
                    if call_full(context, host, call::PATH_3D_AVAILABLE, 0, 0, &input).result != 0 {
                        firing_position.path_distance_to_target = normalize3(&mut target_vector);
                        if evaluation_context.find_path_direction_from_target {
                            firing_position.path_direction_from_target = target_vector;
                        }
                    }
                }
            }
        } else if evaluation_context.target_pathfinding_surface_index != NONE {
            call_full(
                context,
                host,
                call::TARGET_PATH,
                evaluation_context.target_pathfinding_surface_index,
                0,
                &evaluation_context.target_pathfinding_point,
            );
            let want_direction = evaluation_context.find_path_direction_from_target;
            for firing_position in firing_positions.iter_mut() {
                let estimated = call_full(
                    context,
                    host,
                    call::TARGET_DISTANCE,
                    firing_position.surface_index,
                    want_direction as i32,
                    &firing_position.position,
                );
                firing_position.path_distance_to_target = estimated.output[0];
                if want_direction {
                    firing_position.path_direction_from_target =
                        [estimated.output[2], estimated.output[3], estimated.output[4]];
                }
            }
        }
    }

    if !evaluation_context.flying {
        let mut input = [0.0f32; 6];
        input[0] = evaluation_context.maximum_search_range;
        let mut attractor = 0;
        let mut object_index = NONE;
        if evaluation_context.attractor_enable && evaluation_context.has_target {
            if evaluation_context.target_prop_index != NONE {
                object_index = m.i32(prop_get(m, context, evaluation_context.target_prop_index) + p::UNIT_INDEX as u32);
            }
            attractor = 1;
            input[1..4].copy_from_slice(&evaluation_context.target_point);
            input[4] = evaluation_context.attractor_radius;
            input[5] = evaluation_context.attractor_weight;
        } else if danger_type > v::_ACTOR_DANGER_ZONE_NONE as i16
            && m.r32(definition(d::FLAGS2)) & FLAGS2_PATHFINDING_IGNORES_DANGER == 0
        {
            attractor = 2;
            object_index = m.i32(at(a::DANGER_ZONE_OBJECT_INDEX));
            input[1..4].copy_from_slice(&m.vector(at(a::DANGER_ZONE_POSITION)));
            input[4] = m.f32(at(a::DANGER_ZONE_DANGER_RADIUS));
            input[5] = 10.0;
        }
        if call_full(context, host, call::AREA_PATH, attractor, object_index, &input).result != 0 {
            *area_path_state_valid = true;
        }
    }

    let mut any_in_range = false;
    let maximum_search_range = evaluation_context.maximum_search_range;
    for firing_position in firing_positions.iter_mut() {
        if evaluation_context.has_target {
            firing_position.linear_distance_squared_to_target =
                distance_squared(evaluation_context.target_point, firing_position.position);
        }
        let mut from_actor = vector_from_points(body_position, firing_position.position);
        if magnitude_squared(from_actor) < maximum_search_range * maximum_search_range {
            if evaluation_context.flying {
                firing_position.path_closest_approach_to_target =
                    point_to_line_distance_squared(evaluation_context.target_point, body_position, from_actor).sqrt();
                firing_position.path_distance_from_actor = normalize3(&mut from_actor);
                if evaluation_context.find_path_direction_from_actor {
                    firing_position.path_direction_from_actor = from_actor;
                }
            } else {
                let want_direction = evaluation_context.find_path_direction_from_actor;
                let estimated = call_full(
                    context,
                    host,
                    call::AREA_DISTANCE,
                    firing_position.surface_index,
                    want_direction as i32,
                    &firing_position.position,
                );
                firing_position.path_distance_from_actor = estimated.output[0];
                firing_position.path_closest_approach_to_target = estimated.output[1];
                if want_direction {
                    firing_position.path_direction_from_actor = [estimated.output[2], estimated.output[3], estimated.output[4]];
                }
            }
        }
        if firing_position.path_distance_from_actor < maximum_search_range {
            any_in_range = true;
        } else {
            firing_position.valid = false;
        }
    }

    if evaluation_context.allow_outside_range && !any_in_range {
        best_index = call_simple(context, host, call::RANDOM_RANGE, firing_positions.len() as i32) as i16;
        *area_path_state_valid = false;
        clear_discarded_firing_positions(m, context, false);
        // (a draw in range: the game's always is; a check's replay after a
        // difference needn't be)
        let valid = match firing_positions.get_mut(best_index as usize) {
            Some(firing_position) => forced_evaluation(context, host, evaluation_context, firing_position),
            None => false,
        };
        if !valid {
            best_index = NONE as i16;
        }
    } else {
        pre_evaluate(context, host, evaluation_context, &mut firing_positions);
        let mut sorted_indices: Vec<i32> = (0..firing_positions.len() as i32).collect();
        qsort_4byte(&mut sorted_indices, |first, second| {
            compare(&firing_positions[first as usize], &firing_positions[second as usize])
        });
        evaluation_context.post_evaluation_bounded = post_evaluation_bound(context, host, evaluation_context);
        for &sorted_index in &sorted_indices {
            let firing_position = &mut firing_positions[sorted_index as usize];
            if (!firing_position.valid
                || (evaluation_context.post_evaluation_bounded
                    && firing_position.evaluation + evaluation_context.post_evaluation_bound <= best_evaluation))
                && !evaluate_all
            {
                break;
            }
            if firing_position.valid {
                if evaluation_context.has_target {
                    compute_line_of_sight(context, host, evaluation_context, firing_position);
                }
                firing_position.pre_evaluation = firing_position.evaluation;
                if post_evaluate(context, host, evaluation_context, Some(firing_position))
                    && firing_position.evaluation > best_evaluation
                {
                    best_index = sorted_index as i16;
                    best_evaluation = firing_position.evaluation;
                }
            }
        }
    }

    if best_index != NONE as i16 {
        *best_firing_position = firing_positions[best_index as usize];
        best_index = best_firing_position.original_index;
        *current_owner = call_simple(context, host, call::OWNER, best_index as i32);
    }
    best_index
}

fn call_simple_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> AiCall {
    call_full(context, host, kind, a, 0, &[])
}

/// `actor_active_select_firing_position`: the selection of an actor that
/// may move between its searching and its other groups (preferring those it
/// is in), and that keeps a position it found outside its range when there
/// is no other.
pub fn active_select_firing_position(
    context: &AiStepContext,
    host: &impl AiHost,
    evaluation_context: &mut EvaluationContext,
    best_firing_position: &mut FiringPosition,
    current_owner: &mut i32,
    area_path_state_valid: &mut bool,
) -> i16 {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i32(at(a::META_ENCOUNTER_INDEX)) == NONE {
        return NONE as i16;
    }
    let evaluation_mode = evaluation_context.evaluation_mode;
    let currently_allowed_groups = firing_position_group(m, context, evaluation_mode, group_selection::NORMAL);
    let groups_when_not_searching = firing_position_group(m, context, evaluation_mode, group_selection::WHEN_NOT_SEARCHING);
    let mut total_groups = firing_position_group(m, context, evaluation_mode, group_selection::WHEN_SEARCHING);
    total_groups |= groups_when_not_searching;
    if total_groups > currently_allowed_groups {
        evaluation_context.preferred_groups = currently_allowed_groups;
        evaluation_context.preferred_weight = 8.0;
        evaluation_context.allowed_position_mask = total_groups;
    } else {
        evaluation_context.allowed_position_mask = currently_allowed_groups;
    }
    let kept_outside_range = |m: &dyn AiMemory| {
        m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX)) != NONE as i16
            && m.bool(at(a::FIRING_POSITIONS_CURRENT_POSITION_FOUND_OUTSIDE_RANGE))
    };
    evaluation_context.allow_outside_range = !kept_outside_range(m);
    evaluation_context.allow_rejected_positions = true;

    let mut result =
        select_firing_position(context, host, evaluation_context, best_firing_position, current_owner, area_path_state_valid);
    if result != NONE as i16 {
        if !test_flag(currently_allowed_groups, best_firing_position.group_index as i32) {
            let searching = at(a::STATE_SEARCHING);
            m.set_bool(searching, !m.bool(searching));
        }
    } else if kept_outside_range(m) {
        let mut firing_position_index = m.i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX));
        let encounter = encounter_definition(m, context, m.i32(at(a::META_ENCOUNTER_INDEX)));
        let definition = tag_block_element(
            m,
            encounter + encounter_definition::FIRING_POSITIONS as u32,
            firing_position_index as i32,
            fpd::SIZEOF,
        );
        let previous = *best_firing_position;
        *best_firing_position = FiringPosition {
            line_of_sight: LINE_OF_SIGHT_CLEAR,
            path_distance_from_actor: f32::MAX,
            path_distance_to_target: f32::MAX,
            path_closest_approach_to_target: f32::MAX,
            valid: previous.valid,
            rejected: previous.rejected,
            pre_evaluation: previous.pre_evaluation,
            evaluation: previous.evaluation,
            ..FiringPosition::definition(m, definition, firing_position_index)
        };
        best_firing_position.linear_distance_squared_to_target = if evaluation_context.has_target {
            distance_squared(evaluation_context.target_point, best_firing_position.position)
        } else {
            0.0
        };
        if !forced_evaluation(context, host, evaluation_context, best_firing_position) {
            firing_position_index = NONE as i16;
            m.set_i16(at(a::FIRING_POSITIONS_CURRENT_POSITION_INDEX), NONE as i16);
        }
        *current_owner = NONE;
        *area_path_state_valid = false;
        result = firing_position_index;
    }
    result
}

/// `actor_change_firing_position` to the selection's position (NONE: none),
/// with the path search the selection left the game: the actor's position
/// now.
pub fn change_firing_position(
    context: &AiStepContext,
    host: &impl AiHost,
    firing_position_index: i16,
    previous_owner: i32,
    cached_path_available: bool,
) -> i16 {
    call_full(
        context,
        host,
        call::CHANGE,
        firing_position_index as i32,
        previous_owner,
        &[cached_path_available as i32 as f32],
    )
    .result as i16
}

/// The active selection in the mode and the change to it (the fight's and
/// the avoidance's).
pub fn change_to_active_selection(context: &AiStepContext, host: &impl AiHost, evaluation_mode: i16) -> i16 {
    let mut evaluation_context = EvaluationContext::new(evaluation_mode);
    let mut candidate = FiringPosition::default();
    let mut previous_owner = NONE;
    let mut position_valid = false;
    let firing_position_index = active_select_firing_position(
        context,
        host,
        &mut evaluation_context,
        &mut candidate,
        &mut previous_owner,
        &mut position_valid,
    );
    change_firing_position(context, host, firing_position_index, previous_owner, position_valid)
}

/// The guard's selection (of its guarding groups, outside its range if it
/// must) and the change to it.
pub fn change_to_guard_selection(context: &AiStepContext, host: &impl AiHost) -> i16 {
    let m = host.memory();
    let mut evaluation_context = EvaluationContext::new(mode::GUARD);
    evaluation_context.allowed_position_mask = firing_position_group(m, context, mode::GUARD, group_selection::NORMAL);
    evaluation_context.allow_outside_range = true;
    let mut candidate = FiringPosition::default();
    let mut previous_owner = NONE;
    let mut position_valid = false;
    let firing_position_index = select_firing_position(
        context,
        host,
        &mut evaluation_context,
        &mut candidate,
        &mut previous_owner,
        &mut position_valid,
    );
    change_firing_position(context, host, firing_position_index, previous_owner, position_valid)
}

/// The pursuit's selection (actions.c's `_actions_port_handler_call_select_
/// pursuit_position`): a position to look from, twenty units out.
pub fn select_pursuit_position(context: &AiStepContext, host: &impl AiHost, tenacious: bool, last_perceived_time: i32) -> i16 {
    let m = host.memory();
    let target_prop_index = m.i32(context.actor + a::TARGET_TARGET_PROP_INDEX as u32);
    let mut evaluation_context = EvaluationContext::new(mode::PURSUE);
    // (the union's first byte is the orphan's index's)
    evaluation_context.forced_to_flee = target_prop_index as u8 != 0;
    evaluation_context.pursue_last_perceived_time = last_perceived_time;
    evaluation_context.pursue_tenacious = tenacious;
    evaluation_context.find_path_direction_from_target = target_prop_index != NONE;
    evaluation_context.allowed_position_mask = firing_position_group(m, context, mode::PURSUE, group_selection::NORMAL);
    evaluation_context.maximum_search_range = 20.0;
    let mut candidate = FiringPosition::default();
    let mut previous_owner = NONE;
    let mut position_valid = false;
    select_firing_position(context, host, &mut evaluation_context, &mut candidate, &mut previous_owner, &mut position_valid)
}

/// What a flee's search starts from (`struct flee_state_data`'s fields).
#[derive(Debug, Clone, Copy, Default)]
pub struct FleeSearch {
    pub panic_type: i16,
    pub forced_flee_ticks: i16,
    pub flee_from_last_visible_location: bool,
    pub allow_occluded_points: bool,
    pub flee_prop_index: i32,
    pub approach_point: [f32; 3],
}

/// What it finds.
#[derive(Debug, Clone, Copy, Default)]
pub struct FleeFound {
    pub firing_position_index: i16,
    pub found_randomly: bool,
    pub has_approach_point: bool,
    pub approach_point: [f32; 3],
}

/// action_flee.c's `action_flee_find_flee_position`: in a panic, a position
/// away from the actor and its target (drawn by chance if it must be, and
/// drawn towards a target), otherwise cover near it; the change to it, and
/// the point from which the prop it flees would approach it. With
/// `in_actor` the flee's state is the actor's, written as the C writes it.
pub fn find_flee_position(context: &AiStepContext, host: &impl AiHost, search: &FleeSearch, in_actor: bool) -> FleeFound {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let write_index = |index: i16| {
        if in_actor {
            m.set_i16(at(a::STATE_FLEE_FLEE_FIRING_POSITION_INDEX), index);
        }
    };
    let mut evaluation_context = EvaluationContext {
        use_last_visible_target_position: search.flee_from_last_visible_location,
        ..EvaluationContext::default()
    };
    if search.panic_type > 0 {
        evaluation_context.evaluation_mode = mode::PANIC;
        if search.forced_flee_ticks > 0 {
            evaluation_context.allow_rejected_positions = true;
            evaluation_context.allow_outside_range = true;
        }
        evaluation_context.attractor_enable = true;
        evaluation_context.attractor_weight = 10.0;
        evaluation_context.attractor_radius = 6.0;
    } else {
        // (a cover's allow_occluded_points: the union's first byte)
        evaluation_context.forced_to_flee = search.allow_occluded_points;
        evaluation_context.evaluation_mode = mode::COVER;
        let cover_max_distance = m.f32(context.definition + d::DEFENSIVE_COVER_MAX_DISTANCE as u32);
        evaluation_context.maximum_search_range = if cover_max_distance > 0.0 { cover_max_distance } else { 6.0 };
    }
    evaluation_context.allowed_position_mask =
        firing_position_group(m, context, evaluation_context.evaluation_mode, group_selection::NORMAL);
    let mut candidate = FiringPosition::default();
    let mut current_owner = NONE;
    let mut area_path_state_valid = false;
    let mut found = FleeFound { approach_point: search.approach_point, ..FleeFound::default() };
    found.firing_position_index = select_firing_position(
        context,
        host,
        &mut evaluation_context,
        &mut candidate,
        &mut current_owner,
        &mut area_path_state_valid,
    );
    write_index(found.firing_position_index);
    found.firing_position_index =
        change_firing_position(context, host, found.firing_position_index, current_owner, area_path_state_valid);
    write_index(found.firing_position_index);
    found.found_randomly = found.firing_position_index != NONE as i16 && !area_path_state_valid;
    found.has_approach_point = false;
    if in_actor {
        m.set_bool(at(a::STATE_FLEE_FLEE_FIRING_POSITION_FOUND_RANDOMLY), found.found_randomly);
        m.set_bool(at(a::STATE_FLEE_HAS_APPROACH_POINT), false);
    }
    if found.firing_position_index != NONE as i16 && search.flee_prop_index != NONE {
        let prop = prop_get(m, context, search.flee_prop_index);
        if acknowledged(m.i16(prop + p::STATE as u32)) {
            call_simple(context, host, call::FIND_PROP_PATHFINDING_LOCATION, search.flee_prop_index);
        }
        let mut input = [0.0f32; 6];
        input[0..3].copy_from_slice(&found.approach_point);
        input[3..6].copy_from_slice(&candidate.position);
        let approach =
            call_full(context, host, call::FLEE_APPROACH, search.flee_prop_index, candidate.surface_index, &input);
        if approach.result2 != 0 {
            found.has_approach_point = approach.result != 0;
            found.approach_point = approach.vector();
            if in_actor {
                for (k, value) in found.approach_point.iter().enumerate() {
                    m.set_f32(at(a::STATE_FLEE_APPROACH_POINT) + 4 * k as u32, *value);
                }
                m.set_bool(at(a::STATE_FLEE_HAS_APPROACH_POINT), found.has_approach_point);
            }
        }
    }
    if in_actor {
        m.set_bool(at(a::STATE_FLEE_FIND_NEW_FLEE_POSITION), false);
    }
    found
}
