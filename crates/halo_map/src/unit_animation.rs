//! Halo's unit animation state (ROADMAP phase 7, objects and physics):
//! `unit_update_animation` and `unit_animation_set_state` in OpenCE's
//! `source/units/units.c`, with `animation_update_internal` and
//! `animation_choose_random_permutation_internal` from
//! `source/models/model_animations.c`, ported as they are.
//!
//! A unit's animation graph holds, per seat (its stance: asleep, alert,
//! stand, crouch, flee, flaming, or a vehicle's seat) and per weapon class,
//! the animation for each state (idle, turning, moving, airborne,
//! landing, throwing, melee, dying...), each with permutations chosen by
//! weight from the game's random numbers. Each tick the unit picks its
//! stance from the AI's animation state, advances what plays (the base
//! animation, a soft ping, an action, an overlay action), and enters the
//! state its update asked for when the state it is in may be interrupted.
//!
//! What that sets off elsewhere (a grenade's release, melee damage,
//! destroying a dead unit, a limp body, seat changes, the sound an
//! animation plays) is reported as side effects; in a running game an
//! [`UnitAnimationHost`] does them at the moments the C does, the game's
//! random seed the update's at each. The poses the
//! animations give the unit's nodes are in `pose` and `unit_pose`.

use crate::{CacheFile, Result, layout};

/// An animation of a graph, what playing it reads.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GraphAnimation {
    pub frame_count: i16,
    pub loop_frame_index: i16,
    pub key_frame_index: i16,
    pub second_key_frame_index: i16,
    pub next_animation_index: i16,
    pub runtime_parent_animation_index: i16,
    pub runtime_normalized_weight: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeaponClass {
    pub label: String,
    /// Per `_unit_weapon_class_animation`, its animation (NONE for none).
    pub animations: Vec<i16>,
    pub weapon_type_labels: Vec<String>,
    /// Per weapon type, per `_unit_weapon_type_animation`, its animation.
    pub weapon_type_animations: Vec<Vec<i16>>,
    pub aiming_screen_bounds: crate::pose::ScreenBounds,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitSeat {
    pub label: String,
    /// Per `_unit_seat_animation`, its animation (NONE for none).
    pub animations: Vec<i16>,
    pub weapon_classes: Vec<WeaponClass>,
    pub looking_screen_bounds: crate::pose::ScreenBounds,
}

/// An animation graph (`antr` tag), what choosing and advancing a unit's
/// animations reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimationGraph {
    pub unit_seats: Vec<UnitSeat>,
    pub animations: Vec<GraphAnimation>,
}

impl AnimationGraph {
    pub fn load(cache: &CacheFile, datum: u32) -> Result<Self> {
        use layout::{
            animation as a, animation_graph as g, animation_graph_unit_seat as s, animation_graph_weapon_class as w,
            animation_graph_weapon_type as t,
        };
        let root = cache.tag_root(datum, crate::groups::ANIMATION_GRAPH)?;
        let indices = |ptr: crate::Ptr<'_>, rel: usize| -> Result<Vec<i16>> {
            ptr.block(rel, 2)?.iter().map(|p| p.i16(0)).collect()
        };
        let label = |ptr: crate::Ptr<'_>| ptr.c_string(32);
        let mut graph = AnimationGraph::default();
        for seat in root.block(g::UNIT_SEATS, s::SIZEOF)?.iter() {
            let mut weapon_classes = Vec::new();
            for class in seat.block(s::WEAPON_CLASSES, w::SIZEOF)?.iter() {
                let weapon_type_labels =
                    class.block(w::WEAPON_TYPES, t::SIZEOF)?.iter().map(|p| label(p.add(t::LABEL))).collect::<Result<_>>()?;
                let weapon_type_animations =
                    class.block(w::WEAPON_TYPES, t::SIZEOF)?.iter().map(|p| indices(p, t::ANIMATIONS)).collect::<Result<_>>()?;
                weapon_classes.push(WeaponClass {
                    label: label(class.add(w::LABEL))?,
                    animations: indices(class, w::ANIMATIONS)?,
                    weapon_type_labels,
                    weapon_type_animations,
                    aiming_screen_bounds: crate::pose::ScreenBounds::read(class.add(w::AIMING_SCREEN_BOUNDS))?,
                });
            }
            graph.unit_seats.push(UnitSeat {
                label: label(seat.add(s::LABEL))?,
                animations: indices(seat, s::ANIMATIONS)?,
                weapon_classes,
                looking_screen_bounds: crate::pose::ScreenBounds::read(seat.add(s::LOOKING_SCREEN_BOUNDS))?,
            });
        }
        for animation in root.block(g::ANIMATIONS, a::SIZEOF)?.iter() {
            graph.animations.push(GraphAnimation {
                frame_count: animation.i16(a::FRAME_COUNT)?,
                loop_frame_index: animation.i16(a::PRIVATE_LOOP_FRAME_INDEX)?,
                key_frame_index: animation.i16(a::PRIVATE_KEY_FRAME_INDEX)?,
                second_key_frame_index: animation.i16(a::PRIVATE_SECOND_KEY_FRAME_INDEX)?,
                next_animation_index: animation.i16(a::NEXT_ANIMATION_INDEX)?,
                runtime_parent_animation_index: animation.i16(a::RUNTIME_PARENT_ANIMATION_INDEX)?,
                runtime_normalized_weight: animation.f32(a::RUNTIME_NORMALIZED_WEIGHT)?,
            });
        }
        Ok(graph)
    }
}

/// What `unit_update_animation` changes of a unit, laid out as the bridge
/// shares it.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UnitAnimationState {
    pub flags: u16,
    pub aiming_screen_index: i16,
    pub looking_screen_index: i16,
    pub action_animation_index: i16,
    pub action_animation_frame_index: i16,
    pub overlay_action_animation_index: i16,
    pub overlay_action_animation_frame_index: i16,
    pub soft_ping_animation_index: i16,
    pub soft_ping_animation_frame_index: i16,
    pub object_animation_index: i16,
    pub object_animation_frame_index: i16,
    pub interpolation_frame_index: i16,
    pub interpolation_frame_count: i16,
    pub seat_index: i8,
    pub weapon_index: i8,
    pub weapon_type_index: i8,
    pub state: i8,
    pub action: i8,
    pub overlay_action: i8,
    pub base_seat_index: i8,
    pub pad: i8,
    pub pad2: i16,
    pub object_animation_graph_index: u32,
}

const _: () = assert!(std::mem::size_of::<UnitAnimationState>() == 40);

/// What `unit_update_animation` reads besides the state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitAnimationInputs {
    /// The animation update its unit's update made (the state it asks for,
    /// and whether it crouches).
    pub state_desired: i8,
    pub crouching: bool,
    /// The AI's animation state (asleep, alert, ...).
    pub desired_state: i8,
    pub dead: bool,
    pub parented: bool,
    pub player: bool,
    pub force_alert: bool,
    pub flaming: bool,
    /// `magic_base_animation_seat_index` (a debug override for players).
    pub magic_base_seat_index: i16,
    pub magic_seat_index: i8,
    pub at_rest: bool,
    pub biped: bool,
    pub airborne: bool,
    /// The current weapon's label ("unarmed" for none).
    pub weapon_label: String,
    pub random_seed: u32,
    /// The unit definition's "destroyed after dying" flag.
    pub destroyed_after_dying: bool,
    /// The biped definition's "has no dying airborne" flag.
    pub has_no_dying_airborne: bool,
}

/// A running game, as a unit's animation update calls it: the game's
/// random seed (which the permutations draw from, as do the effects), and
/// each side effect ([`side_effects`]'s bit) as it happens, with the
/// update's state (the host writes it to the unit first, and reads back what
/// the effect changed). An effect returns whether the unit is still there
/// (destroying a dead one deletes it).
pub trait UnitAnimationHost {
    fn random_seed(&self) -> u32;
    fn set_random_seed(&self, seed: u32);
    fn effect(&self, state: &mut UnitAnimationState, effect: u32) -> bool;
}

/// The host as an update calls it: no more calls once the unit is gone.
struct Effects<'a> {
    host: Option<&'a dyn UnitAnimationHost>,
    flags: u32,
}

impl Effects<'_> {
    fn raise(&mut self, effect: u32, random: &mut Random, state: &mut UnitAnimationState) {
        self.flags |= effect;
        if let Some(host) = self.host {
            host.set_random_seed(random.seed);
            let there = host.effect(state, effect);
            random.seed = host.random_seed();
            if !there {
                self.host = None;
            }
        }
    }
}

/// `unit_update_animation`'s side effects (the bridge's bits).
pub mod side_effects {
    pub const GRENADE_RELEASED: u32 = 1;
    pub const MELEE_DAMAGE: u32 = 2;
    pub const DESTROYED: u32 = 4;
    pub const LIMP_BODY: u32 = 8;
    pub const SEAT_EXITED: u32 = 16;
    pub const SEAT_ENTERED: u32 = 32;
    pub const GRENADE_RELEASED_CHANGING_STATE: u32 = 64;
}

/// `animation_update_result`.
pub mod update_result {
    pub const RUNNING: i16 = 0;
    pub const KEY_FRAME: i16 = 1;
    pub const WILL_RESTART_ON_NEXT_FRAME: i16 = 2;
    pub const RESTARTED: i16 = 3;
    pub const LOOPED: i16 = 4;
}

const NONE: i16 = -1;
const UNIT_ANIMATION_SHOWING_ACCELERATION: u16 = 1 << 1;
const UNIT_ANIMATION_IGNORE_TRANSLATION: u16 = 1 << 2;
const BASE_SEAT_LABELS: [&str; 6] = ["asleep", "alert", "stand", "crouch", "flee", "flaming"];
const NUMBER_OF_UNIT_ANIMATION_STATES: i16 = 7;
const SEAT_ANIMATION_LOOKING: usize = 9;

/// `_unit_state`s.
#[allow(dead_code)]
mod state {
    pub const IDLE: i8 = 0;
    pub const GESTURE: i8 = 1;
    pub const TURN_LEFT: i8 = 2;
    pub const TURN_RIGHT: i8 = 3;
    pub const MOVE_FRONT: i8 = 4;
    pub const MOVE_BACK: i8 = 5;
    pub const MOVE_LEFT: i8 = 6;
    pub const MOVE_RIGHT: i8 = 7;
    pub const STUNNED_MOVE_FRONT: i8 = 8;
    pub const STUNNED_MOVE_RIGHT: i8 = 11;
    pub const SLIDE_FRONT: i8 = 12;
    pub const SLIDE_RIGHT: i8 = 15;
    pub const FLYING_FRONT: i8 = 16;
    pub const FLYING_RIGHT: i8 = 19;
    pub const AIRBORNE: i8 = 20;
    pub const LAND_SOFT: i8 = 21;
    pub const LAND_HARD: i8 = 22;
    pub const HARD_PING: i8 = 23;
    pub const DYING_AIRBORNE: i8 = 24;
    pub const DYING: i8 = 25;
    pub const ENTERING_SEAT: i8 = 26;
    pub const EXITING_SEAT: i8 = 27;
    pub const USER_ANIMATION: i8 = 28;
    pub const AI_IMPULSE: i8 = 29;
    pub const MELEE_ATTACK: i8 = 30;
    pub const MELEE_AIRBORNE: i8 = 31;
    pub const MELEE_CONTINUOUS: i8 = 32;
    pub const THROW_GRENADE: i8 = 33;
    pub const RESURRECT_FRONT: i8 = 34;
    pub const RESURRECT_BACK: i8 = 35;
    pub const FEEDING: i8 = 36;
    pub const OPENING: i8 = 37;
    pub const CLOSING: i8 = 38;
    pub const LEAP_START: i8 = 39;
    pub const LEAP_AIRBORNE: i8 = 40;
    pub const LEAP_MELEE: i8 = 41;
    pub const HOVERING: i8 = 43;
}

/// The global random seed as the game draws from it.
struct Random {
    seed: u32,
}

impl Random {
    /// `real_seed_random`.
    fn real(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 16) as f32 / 65535.0
    }
}

/// `animation_choose_random_permutation_internal`: one of an animation's
/// permutations by weight (a random number is drawn even for none).
fn choose_random_permutation(graph: &AnimationGraph, random: &mut Random, animation_index: i16) -> i16 {
    let random = random.real();
    let first_animation_index = animation_index;
    let mut animation_index = animation_index;
    let count = graph.animations.len();
    let valid = |index: i16| index >= 0 && (index as usize) < count;
    let mut permutation_count = 0usize;
    while animation_index != NONE {
        if !valid(animation_index) || {
            let over = permutation_count >= count;
            permutation_count += 1;
            over
        } {
            animation_index = if valid(first_animation_index) { first_animation_index } else { NONE };
            break;
        }
        let animation = &graph.animations[animation_index as usize];
        if random <= animation.runtime_normalized_weight {
            break;
        }
        animation_index = animation.next_animation_index;
    }
    animation_index
}

/// `animation_update_internal` affecting the game's state (its random
/// numbers the global seed's): a frame further, and what that came to
/// (`update_result`).
pub fn animation_update(graph: Option<&AnimationGraph>, seed: &mut u32, index: &mut i16, frame_index: &mut i16) -> i16 {
    let mut random = Random { seed: *seed };
    let result = update_internal(graph, &mut random, index, frame_index);
    *seed = random.seed;
    result
}

/// `animation_update_internal`: a frame further, and what that came to.
fn update_internal(graph: Option<&AnimationGraph>, random: &mut Random, index: &mut i16, frame_index: &mut i16) -> i16 {
    let Some(graph) = graph else { return update_result::RUNNING };
    let Some(&animation) = (*index >= 0).then(|| graph.animations.get(*index as usize)).flatten() else {
        return update_result::RUNNING;
    };
    *frame_index = frame_index.wrapping_add(1);
    if *frame_index >= animation.frame_count {
        if animation.loop_frame_index > 0 {
            let last = animation.frame_count - 1;
            *frame_index = if animation.loop_frame_index > last { last } else { animation.loop_frame_index };
            update_result::LOOPED
        } else {
            *index = choose_random_permutation(graph, random, animation.runtime_parent_animation_index);
            *frame_index = 0;
            update_result::RESTARTED
        }
    } else if *frame_index + 1 == animation.frame_count && animation.loop_frame_index == 0 {
        update_result::WILL_RESTART_ON_NEXT_FRAME
    } else if *frame_index == animation.key_frame_index || *frame_index == animation.second_key_frame_index {
        update_result::KEY_FRAME
    } else {
        update_result::RUNNING
    }
}

/// `unit_animation_state_interruptable`.
fn interruptable(current: i8, desired: i8) -> bool {
    use state::*;
    match current {
        AI_IMPULSE | MELEE_ATTACK | MELEE_AIRBORNE | THROW_GRENADE | RESURRECT_FRONT | RESURRECT_BACK | LEAP_START | LEAP_MELEE => {
            desired == HARD_PING
        }
        DYING_AIRBORNE | DYING => (DYING_AIRBORNE..=DYING).contains(&desired),
        TURN_LEFT | TURN_RIGHT | OPENING | CLOSING => desired != IDLE,
        HARD_PING | ENTERING_SEAT | EXITING_SEAT | USER_ANIMATION => false,
        _ => true,
    }
}

/// `unit_animation_state_loops`.
fn state_loops(current: i8) -> bool {
    use state::*;
    !matches!(
        current,
        GESTURE
            | TURN_LEFT
            | TURN_RIGHT
            | HARD_PING
            | ENTERING_SEAT
            | EXITING_SEAT
            | USER_ANIMATION
            | AI_IMPULSE
            | MELEE_ATTACK
            | MELEE_AIRBORNE
            | THROW_GRENADE
            | RESURRECT_FRONT
            | RESURRECT_BACK
            | LEAP_START
            | LEAP_MELEE
    )
}

/// `unit_animation_overlay_action_loops`.
fn overlay_action_loops(current: i8) -> bool {
    (state::TURN_RIGHT..=state::MOVE_FRONT).contains(&current)
}

/// `unit_animation_state_can_be_entered_without_animation`.
fn can_be_entered_without_animation(new_state: i8) -> bool {
    use state::*;
    !matches!(new_state, MELEE_ATTACK | MELEE_AIRBORNE | MELEE_CONTINUOUS | THROW_GRENADE | LEAP_START | LEAP_MELEE)
}

/// `unit_animation_compute_interpolation_frame_count`.
fn interpolation_frame_count(new_state: i8, old_state: i8) -> i16 {
    use state::*;
    let still = |s: i8| s == IDLE || s == TURN_LEFT || s == TURN_RIGHT;
    let mut result = 6;
    if still(new_state) && still(old_state) {
        result = 1;
    }
    if new_state == LAND_HARD || new_state == LAND_SOFT {
        result = 2;
    }
    result
}

/// `unit_animation_state_get_aiming_screen_index`: aiming still (25) or
/// moving (26), NONE for neither.
fn aiming_screen_index(s: i8) -> i16 {
    use state::*;
    match s {
        IDLE | TURN_LEFT | TURN_RIGHT | FLYING_FRONT..=FLYING_RIGHT | AIRBORNE | LAND_SOFT | LAND_HARD | OPENING | CLOSING => 25,
        MOVE_FRONT..=STUNNED_MOVE_RIGHT | SLIDE_FRONT..=SLIDE_RIGHT => 26,
        _ => NONE,
    }
}

/// The weapon class (and seat) animation a state plays.
fn state_animation_slots(new_state: i8) -> (i16, i16) {
    use state::*;
    let weapon_class = match new_state {
        IDLE => 0,
        GESTURE => 1,
        TURN_LEFT => 2,
        TURN_RIGHT => 3,
        MOVE_FRONT..=MOVE_RIGHT => 8 + (new_state - MOVE_FRONT) as i16,
        STUNNED_MOVE_FRONT..=STUNNED_MOVE_RIGHT => 35 + (new_state - STUNNED_MOVE_FRONT) as i16,
        SLIDE_FRONT..=SLIDE_RIGHT => 12 + (new_state - SLIDE_FRONT) as i16,
        AIRBORNE => 16,
        LAND_SOFT => 17,
        LAND_HARD => 18,
        THROW_GRENADE => 20,
        MELEE_ATTACK => 39,
        MELEE_AIRBORNE => 42,
        MELEE_CONTINUOUS => 46,
        RESURRECT_FRONT => 44,
        RESURRECT_BACK => 45,
        FEEDING => 47,
        LEAP_START => 48,
        LEAP_AIRBORNE => 49,
        LEAP_MELEE => 50,
        _ => NONE,
    };
    let seat = match new_state {
        FLYING_FRONT..=FLYING_RIGHT => 23 + (new_state - FLYING_FRONT) as i16,
        DYING_AIRBORNE => 0,
        DYING => 1,
        OPENING => 27,
        CLOSING => 28,
        HOVERING => 29,
        _ => NONE,
    };
    (weapon_class, seat)
}

fn slot(animations: &[i16], index: i16) -> i16 {
    if index >= 0 && (index as usize) < animations.len() { animations[index as usize] } else { NONE }
}

/// `object_start_interpolation`'s counters.
fn start_interpolation(s: &mut UnitAnimationState, frame_count: i16) {
    if frame_count as i32 >= s.interpolation_frame_count as i32 - s.interpolation_frame_index as i32 {
        s.interpolation_frame_index = 0;
        s.interpolation_frame_count = frame_count;
    }
}

/// `unit_set_or_test_seat_and_weapon_label` (setting): the last seat and
/// weapon type matching the labels.
fn set_seat_and_weapon_label(graph: &AnimationGraph, s: &mut UnitAnimationState, seat_label: &str, weapon_label: &str) {
    for (seat_index, seat) in graph.unit_seats.iter().enumerate() {
        if !seat_label.eq_ignore_ascii_case(&seat.label) {
            continue;
        }
        for (class_index, class) in seat.weapon_classes.iter().enumerate() {
            for (type_index, type_label) in class.weapon_type_labels.iter().enumerate() {
                if (weapon_label == "unarmed" && type_label.is_empty()) || weapon_label.eq_ignore_ascii_case(type_label) {
                    let showing_acceleration =
                        slot(&seat.animations, 2) != NONE || slot(&seat.animations, 3) != NONE || slot(&seat.animations, 4) != NONE;
                    if s.state != state::USER_ANIMATION {
                        s.state = NONE as i8;
                    }
                    s.seat_index = seat_index as i8;
                    s.base_seat_index =
                        BASE_SEAT_LABELS.iter().position(|l| l.eq_ignore_ascii_case(seat_label)).map_or(NONE as i8, |i| i as i8);
                    s.weapon_index = class_index as i8;
                    s.weapon_type_index = type_index as i8;
                    if showing_acceleration {
                        s.flags |= UNIT_ANIMATION_SHOWING_ACCELERATION;
                    } else {
                        s.flags &= !UNIT_ANIMATION_SHOWING_ACCELERATION;
                    }
                    break;
                }
            }
        }
    }
}

/// `unit_animation_set_state`: the state's animation (a permutation of
/// the weapon class's, or the seat's), its aiming screen, and an
/// interpolation into them. False when the state needs an animation the
/// graph lacks.
fn set_state(
    graph: &AnimationGraph,
    graph_index: u32,
    random: &mut Random,
    s: &mut UnitAnimationState,
    new_state: i8,
    effects: &mut Effects<'_>,
) -> bool {
    let empty_seat = UnitSeat::default();
    let empty_class = WeaponClass::default();
    let seat = (s.seat_index >= 0).then(|| graph.unit_seats.get(s.seat_index as usize)).flatten().unwrap_or(&empty_seat);
    let class = (s.weapon_index >= 0).then(|| seat.weapon_classes.get(s.weapon_index as usize)).flatten().unwrap_or(&empty_class);
    let old_state_is_none = s.state == NONE as i8;
    let mut changed_state = false;
    let mut frames = 0i16;

    if old_state_is_none || new_state != s.state {
        if s.state == state::THROW_GRENADE {
            effects.raise(side_effects::GRENADE_RELEASED_CHANGING_STATE, random, s);
        }
        let (class_slot, seat_slot) = state_animation_slots(new_state);
        let animation_index = if class_slot != NONE {
            slot(&class.animations, class_slot)
        } else if seat_slot != NONE {
            slot(&seat.animations, seat_slot)
        } else {
            NONE
        };
        if animation_index == NONE && !can_be_entered_without_animation(new_state) {
            return false;
        }
        let animation_index = choose_random_permutation(graph, random, animation_index);
        // (unit_set_animation)
        s.object_animation_graph_index = graph_index;
        s.object_animation_index = animation_index;
        s.object_animation_frame_index = 0;
        frames = interpolation_frame_count(new_state, s.state);
        changed_state = true;
    }

    let aiming = aiming_screen_index(new_state);
    if old_state_is_none || aiming != aiming_screen_index(s.state) {
        let animation_index = slot(&class.animations, aiming);
        s.aiming_screen_index = choose_random_permutation(graph, random, animation_index);
        frames = 6;
        changed_state = true;
        if old_state_is_none {
            let animation_index = slot(&seat.animations, SEAT_ANIMATION_LOOKING as i16);
            s.looking_screen_index = choose_random_permutation(graph, random, animation_index);
        }
    }
    if changed_state {
        start_interpolation(s, frames);
    }
    s.state = new_state;
    true
}

/// `unit_update_animation`: returns what it returned (1 for a leap's start
/// done: the unit jumps) and its side effects. `unit_graph` is the unit
/// definition's graph; `graph` gives a graph by its tag index (the object's
/// animation may be another graph's).
pub fn unit_update_animation<'g>(
    unit_graph_index: u32,
    graph: &dyn Fn(u32) -> Option<&'g AnimationGraph>,
    inputs: &UnitAnimationInputs,
    s: &mut UnitAnimationState,
    host: Option<&dyn UnitAnimationHost>,
) -> (i16, u32) {
    let unit_graph = graph(unit_graph_index);
    let mut random = Random { seed: host.map_or(inputs.random_seed, |h| h.random_seed()) };
    let mut effects = Effects { host, flags: 0 };
    let mut desired_state = inputs.state_desired;
    let mut result = 0i16;
    let mut apply_state = false;

    if !inputs.parented && !inputs.dead {
        let mut desired_base_seat_index: i16 = match inputs.desired_state {
            0 => 0,
            1 | 2 => 1,
            3 => i16::from(inputs.crouching) + 2,
            4 => 2,
            5 => 4,
            6 => 5,
            _ => NONE,
        };
        if inputs.player && inputs.magic_base_seat_index != NONE {
            desired_base_seat_index = if inputs.magic_base_seat_index < 0 {
                0
            } else if inputs.magic_base_seat_index > NUMBER_OF_UNIT_ANIMATION_STATES - 1 {
                NUMBER_OF_UNIT_ANIMATION_STATES - 1
            } else {
                inputs.magic_base_seat_index
            };
        }
        if inputs.magic_seat_index != NONE as i8 {
            desired_base_seat_index = inputs.magic_seat_index as i16;
        }
        if inputs.force_alert {
            desired_base_seat_index = 1;
        }
        if inputs.flaming {
            desired_base_seat_index = 5;
        }
        if s.base_seat_index as i16 != desired_base_seat_index
            && interruptable(s.state, desired_state)
            && let (Some(unit_graph), Some(label)) =
                (unit_graph, (desired_base_seat_index >= 0).then(|| BASE_SEAT_LABELS.get(desired_base_seat_index as usize)).flatten())
        {
            set_seat_and_weapon_label(unit_graph, s, label, &inputs.weapon_label);
        }
    }

    if s.soft_ping_animation_index != NONE {
        let r = update_internal(unit_graph, &mut random, &mut s.soft_ping_animation_index, &mut s.soft_ping_animation_frame_index);
        if r == update_result::WILL_RESTART_ON_NEXT_FRAME {
            s.soft_ping_animation_index = NONE;
        }
    }

    if s.object_animation_index != NONE {
        let r = update_internal(
            graph(s.object_animation_graph_index),
            &mut random,
            &mut s.object_animation_index,
            &mut s.object_animation_frame_index,
        );
        if r == update_result::KEY_FRAME {
            match s.state {
                state::THROW_GRENADE => effects.raise(side_effects::GRENADE_RELEASED, &mut random, s),
                state::MELEE_ATTACK | state::MELEE_AIRBORNE | state::LEAP_MELEE => {
                    effects.raise(side_effects::MELEE_DAMAGE, &mut random, s)
                }
                _ => {}
            }
        } else if r == update_result::WILL_RESTART_ON_NEXT_FRAME {
            match s.state {
                state::LEAP_START => {
                    result = 1;
                    desired_state = state::LEAP_AIRBORNE;
                }
                state::DYING => {
                    let destroy = inputs.destroyed_after_dying
                        && (inputs.at_rest || (inputs.biped && (!inputs.airborne || inputs.has_no_dying_airborne)));
                    if destroy {
                        effects.raise(side_effects::DESTROYED, &mut random, s);
                    } else {
                        if inputs.biped {
                            effects.raise(side_effects::LIMP_BODY, &mut random, s);
                        }
                        s.flags |= UNIT_ANIMATION_IGNORE_TRANSLATION;
                        s.object_animation_frame_index -= 1;
                    }
                }
                state::EXITING_SEAT => effects.raise(side_effects::SEAT_EXITED, &mut random, s),
                state::OPENING | state::CLOSING => s.object_animation_frame_index -= 1,
                state::ENTERING_SEAT => effects.raise(side_effects::SEAT_ENTERED, &mut random, s),
                _ => {}
            }
            if !state_loops(s.state) {
                apply_state = true;
            }
        }
    }

    if s.action_animation_index != NONE {
        let r = update_internal(unit_graph, &mut random, &mut s.action_animation_index, &mut s.action_animation_frame_index);
        if r == update_result::WILL_RESTART_ON_NEXT_FRAME {
            start_interpolation(s, 6);
            s.action = 0;
            s.action_animation_index = NONE;
        }
    }

    if s.overlay_action_animation_index != NONE {
        let r = update_internal(
            unit_graph,
            &mut random,
            &mut s.overlay_action_animation_index,
            &mut s.overlay_action_animation_frame_index,
        );
        if (r == update_result::WILL_RESTART_ON_NEXT_FRAME || r == update_result::LOOPED) && !overlay_action_loops(s.state) {
            s.overlay_action = 0;
            s.overlay_action_animation_index = NONE;
        }
    }

    if (apply_state || (desired_state != s.state && interruptable(s.state, desired_state)))
        && let Some(unit_graph) = unit_graph
    {
        set_state(unit_graph, unit_graph_index, &mut random, s, desired_state, &mut effects);
    }
    if let Some(host) = effects.host {
        host.set_random_seed(random.seed);
    }
    (result, effects.flags)
}

/// The unit's actions (`_unit_animation_action`).
pub mod action {
    pub const NONE: i8 = 0;
    pub const DISARM: i8 = 1;
    pub const DROP: i8 = 2;
    pub const READY: i8 = 3;
    pub const PUT_AWAY: i8 = 4;
    pub const PRIMARY_RELOAD: i8 = 5;
    pub const SECONDARY_RELOAD: i8 = 6;
    pub const MELEE: i8 = 7;
    pub const THROW_GRENADE: i8 = 8;
}

// `_unit_weapon_class_animation`s the actions play
const WEAPON_CLASS_ANIMATION_THROW_GRENADE: i16 = 20;
const WEAPON_CLASS_ANIMATION_DISARM: i16 = 21;
const WEAPON_CLASS_ANIMATION_DROP: i16 = 22;
const WEAPON_CLASS_ANIMATION_READY: i16 = 23;
const WEAPON_CLASS_ANIMATION_PUT_AWAY: i16 = 24;

/// The port's `unit_standing_weapon_type_animation`: the standing seat's
/// animation for the weapon class and type of these labels.
fn standing_weapon_type_animation(graph: &AnimationGraph, class_label: &str, type_label: &str, weapon_type_animation_index: i16) -> i16 {
    for seat in graph.unit_seats.iter().filter(|seat| seat.label.eq_ignore_ascii_case("stand")) {
        for class in seat.weapon_classes.iter().filter(|class| class.label.eq_ignore_ascii_case(class_label)) {
            for (label, animations) in class.weapon_type_labels.iter().zip(&class.weapon_type_animations) {
                if label.eq_ignore_ascii_case(type_label) && (weapon_type_animation_index as usize) < animations.len() {
                    return animations[weapon_type_animation_index as usize];
                }
            }
        }
    }
    NONE
}

/// `unit_animation_start_action`: the action's animation (by the unit's
/// seat, weapon class and weapon type), a permutation of it drawn from the
/// game's random numbers, from its first frame; or, for no action, none.
/// `enhanced` is `unit_animation_enhanced` (a seat with no reload borrows
/// the standing one's).
pub fn unit_animation_start_action(graph: &AnimationGraph, enhanced: bool, random_seed: &mut u32, state: &mut UnitAnimationState, action: i8) {
    if action == action::NONE {
        state.action = 0;
        state.action_animation_index = NONE;
        return;
    }
    let Some(seat) = graph.unit_seats.get(state.seat_index as usize) else { return };
    let Some(class) = seat.weapon_classes.get(state.weapon_index as usize) else { return };
    let weapon_type = state.weapon_type_index as usize;
    let (Some(type_label), Some(type_animations)) = (class.weapon_type_labels.get(weapon_type), class.weapon_type_animations.get(weapon_type)) else {
        return;
    };
    let mut weapon_class_animation_index = NONE;
    let mut weapon_type_animation_index = NONE;
    let mut animation_index = NONE;
    match action {
        action::DISARM => weapon_class_animation_index = WEAPON_CLASS_ANIMATION_DISARM,
        action::DROP => weapon_class_animation_index = WEAPON_CLASS_ANIMATION_DROP,
        action::READY => weapon_class_animation_index = WEAPON_CLASS_ANIMATION_READY,
        action::PUT_AWAY => weapon_class_animation_index = WEAPON_CLASS_ANIMATION_PUT_AWAY,
        action::THROW_GRENADE => weapon_class_animation_index = WEAPON_CLASS_ANIMATION_THROW_GRENADE,
        5 | 6 | 7 | 9 => {
            weapon_type_animation_index = match action {
                5 => 0,
                6 => 1,
                7 => 8,
                _ => 9,
            };
            animation_index = slot(type_animations, weapon_type_animation_index);
        }
        _ => {}
    }
    if weapon_class_animation_index != NONE {
        animation_index = slot(&class.animations, weapon_class_animation_index);
    }
    if animation_index == NONE && (weapon_type_animation_index == 0 || weapon_type_animation_index == 1) && enhanced {
        animation_index = standing_weapon_type_animation(graph, &class.label, type_label, weapon_type_animation_index);
    }
    let interpolation_frame_count = if action == action::MELEE { 0 } else { 6 };
    if animation_index != NONE {
        if interpolation_frame_count > 0 {
            start_interpolation(state, interpolation_frame_count);
        }
        let mut random = Random { seed: *random_seed };
        state.action_animation_index = choose_random_permutation(graph, &mut random, animation_index);
        *random_seed = random.seed;
        state.action_animation_frame_index = 0;
        state.action = action;
    }
}
