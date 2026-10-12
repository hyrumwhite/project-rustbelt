//! props.c: an actor's props, the units it knows of, kept in the game's
//! `prop_data` array and threaded through the actor's list from
//! `meta.first_prop_index`. Making one for a unit (replacing the actor's
//! least wanted prop when it has too many), giving one up, and finding the
//! one it acts on for a unit.

use crate::ai_actor::{AiHost, AiMemoryExt, AiStepContext, NONE, PropIterator, prop_get};
use crate::layout::{actor_datum as a, ai_values as v, prop_datum as p, unit_datum as u};

/// The game's calls props.c makes (halo_sim.h's `_actor_perception_call_*`,
/// shared with the perception update's).
pub mod call {
    /// `actor_switch_props(actor, a, b)`.
    pub const SWITCH_PROPS: i32 = 13;
    /// The unit `a` as a new prop sees it: whether it's the actor's enemy
    /// (bit 0), ally (bit 1), an ally whose status changed (bit 2), dead
    /// (bit 3), done feigning death (bit 4) and a player's (bit 5), its team
    /// from bit 16, the result; its definition's danger radius `output[0]`.
    pub const PROP_UNIT_DETAILS: i32 = 83;
    /// The warning (at most each thirty seconds) that the props are full.
    pub const PROPS_FULL_WARNING: i32 = 84;
}

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32) -> crate::ai_actor::AiCall {
    let mut call = crate::ai_actor::AiCall { kind, actor: context.actor_index, a, b, ..Default::default() };
    host.call(&mut call);
    call
}

fn orphaned(state: i16) -> bool {
    state >= v::_PROP_STATE_UNINSPECTED_ORPHAN as i16 && state <= v::_PROP_STATE_INSPECTED_ORPHAN as i16
}

/// props.c's `prop_remove`: the prop taken out of the actor's list.
fn prop_remove(context: &AiStepContext, host: &impl AiHost, prop_index: i32) {
    let m = host.memory();
    let first = context.actor + a::META_FIRST_PROP_INDEX as u32;
    let mut prop = prop_get(m, context, m.i32(first));
    if m.i32(first) != prop_index {
        let mut previous_next;
        loop {
            previous_next = prop + p::NEXT_PROP_INDEX as u32;
            prop = prop_get(m, context, m.i32(previous_next));
            if prop == 0 || m.i32(previous_next) == prop_index {
                break;
            }
        }
        if prop != 0 {
            m.w32(previous_next, m.r32(prop + p::NEXT_PROP_INDEX as u32));
        }
    } else {
        m.w32(first, m.r32(prop + p::NEXT_PROP_INDEX as u32));
    }
}

/// props.c's `prop_delete`: taken out of the actor's list and freed.
pub fn prop_delete(context: &AiStepContext, host: &impl AiHost, prop_index: i32) {
    prop_remove(context, host, prop_index);
    crate::data_array::datum_delete(host.memory(), context.prop_data, prop_index);
}

/// props.c's `prop_get_active_by_unit_index`: the actor's prop (acknowledged
/// or an orphan) of the unit, or of its swarm.
pub fn prop_get_active_by_unit_index(context: &AiStepContext, host: &impl AiHost, unit_index: i32) -> i32 {
    let m = host.memory();
    let unit = crate::ai_actor::object_get(m, context, unit_index).unwrap_or(0);
    let swarm_actor_index = m.i32(unit + u::UNIT_SWARM_ACTOR_INDEX as u32);
    let target_actor_index =
        if swarm_actor_index != NONE { swarm_actor_index } else { m.i32(unit + u::UNIT_ACTOR_INDEX as u32) };
    let mut iterator = PropIterator::new(m, context);
    while let Some(prop) = iterator.next(m, context) {
        let state = m.i16(prop + p::STATE as u32);
        if state >= v::_PROP_STATE_UNACKNOWLEDGED as i16 && state <= v::_PROP_STATE_BECOMING_ACKNOWLEDGED as i16 {
            continue;
        }
        let actor_index = m.i32(prop + p::ACTOR_INDEX as u32);
        if m.i32(prop + p::UNIT_INDEX as u32) == unit_index
            || (m.bool(prop + p::SWARM as u32) && actor_index != NONE && actor_index == target_actor_index)
        {
            return iterator.index;
        }
    }
    NONE
}

/// props.c's `prop_add`: a new prop (NONE: the array was full, warned) of
/// the unit, set up and put first in the actor's list.
fn prop_add(context: &AiStepContext, host: &impl AiHost, unit_index: i32, prop_index: i32) {
    let m = host.memory();
    if prop_index == NONE {
        call_full(context, host, call::PROPS_FULL_WARNING, 0, 0);
        return;
    }
    let prop = prop_get(m, context, prop_index);
    let field = |offset: usize| prop + offset as u32;
    m.w32(field(p::OWNER_ACTOR_INDEX), context.actor_index as u32);
    m.set_i16(field(p::UNIT_EFFECT), NONE as i16);
    m.set_i16(field(p::TICKS_SINCE_DAMAGE), NONE as i16);
    m.w32(field(p::UNIT_INDEX), unit_index as u32);
    m.set_bool(field(p::CURRENTLY_DAMAGING_ME), false);
    m.set_f32(field(p::DAMAGE_INFLICTED_ON_ME), 0.0);
    m.set_i16(field(p::TICKS_SINCE_DEFINITELY_LOCATED), NONE as i16);
    m.set_bool(field(p::DEFINITELY_LOCATED), false);
    m.w32(field(p::DEFINITE_KNOWLEDGE_SOURCE_ACTOR), NONE as u32);
    m.w32(field(p::LAST_PERCEIVED_TIME), NONE as u32);
    m.w32(field(p::LAST_VISIBLE_TIME), NONE as u32);
    m.set_bool(field(p::ORPHAN_CORPSE_CHEATED), false);
    m.w32(field(p::ACTOR_INDEX), NONE as u32);
    m.w32(field(p::ORPHAN_PROP_INDEX), NONE as u32);
    m.set_i16(field(p::REQUIRED_TICKS), 0);
    m.w32(field(p::LAST_UNREACHABLE_TIME), NONE as u32);

    if unit_index != NONE {
        let details = call_full(context, host, call::PROP_UNIT_DETAILS, unit_index, 0);
        let bits = details.result;
        let unit = crate::ai_actor::object_get(m, context, unit_index).unwrap_or(0);
        m.set_i16(field(p::TEAM_INDEX), (bits >> 16) as i16);
        m.set_bool(field(p::ENEMY), bits & 1 != 0);
        m.set_bool(field(p::ALLY), bits & 2 != 0);
        m.set_bool(field(p::ALLY_STATUS_CHANGED), bits & 4 != 0);
        let dead = bits & 8 != 0;
        m.set_bool(field(p::DEAD), dead);
        m.set_f32(field(p::SUICIDE_RADIUS), details.output[0]);
        m.set_bool(field(p::REALLY_DEAD), dead && bits & 16 != 0);
        m.set_i16(field(p::DEAD_TICKS), if dead { 1000 } else { 0 });
        let player = bits & 32 != 0;
        m.set_bool(field(p::PLAYER), player);
        let swarm_actor_index = m.i32(unit + u::UNIT_SWARM_ACTOR_INDEX as u32);
        if swarm_actor_index != NONE {
            m.set_bool(field(p::SWARM), true);
            m.w32(field(p::ACTOR_INDEX), swarm_actor_index as u32);
            m.w32(field(p::SWARM_UNIT_SELECTED_TIME), context.game_time as u32);
        } else {
            m.w32(field(p::ACTOR_INDEX), m.r32(unit + u::UNIT_ACTOR_INDEX as u32));
        }
        let prop_type = if player {
            6
        } else {
            let actor_index = m.i32(field(p::ACTOR_INDEX));
            match (actor_index != NONE).then(|| crate::ai_actor::datum_get(m, context.actor_data, actor_index)).flatten() {
                Some(prop_actor) => m.i16(prop_actor + a::META_TYPE as u32),
                None => NONE as i16,
            }
        };
        m.set_i16(field(p::TYPE), prop_type);
    }

    let first = context.actor + a::META_FIRST_PROP_INDEX as u32;
    m.w32(field(p::NEXT_PROP_INDEX), m.r32(first));
    m.w32(first, prop_index as u32);
}

/// props.c's `prop_new_unacknowledged`: a prop of the unit, in the place of
/// the nearest prop the actor no longer wants, or else (when it has six
/// wanted enemies or four friends of its kind) the nearest it would give up
/// when crowded; or else a new one.
pub fn prop_new_unacknowledged(context: &AiStepContext, host: &impl AiHost, unit_index: i32, enemy: bool) -> i32 {
    let m = host.memory();
    let mut worst_prop_index = NONE;
    let mut worst_required_prop_index = NONE;
    let mut worst_distance = f32::MAX;
    let mut worst_required_distance = f32::MAX;
    let mut required_prop_count: i16 = 0;
    let mut iterator = PropIterator::new(m, context);
    while let Some(prop) = iterator.next(m, context) {
        let field = |offset: usize| prop + offset as u32;
        if orphaned(m.i16(field(p::STATE))) || m.i32(field(p::ORPHAN_PROP_INDEX)) != NONE {
            continue;
        }
        let distance = m.f32(field(p::DISTANCE));
        let (desire, replace) = crate::actor_perception::desire_prop(
            context,
            host,
            NONE as i16,
            m.i32(field(p::UNIT_INDEX)),
            m.i32(field(p::ACTOR_INDEX)),
            m.bool(field(p::IN_USE)),
            m.bool(field(p::PLAYER)),
            m.bool(field(p::ENEMY)),
            m.bool(field(p::DEAD)),
            m.i16(field(p::DEAD_TICKS)),
            m.f32(field(p::SUICIDE_RADIUS)),
            distance * distance,
            m.i16(field(p::REQUIRED_TICKS)),
        );
        if !desire {
            if distance < worst_distance {
                worst_prop_index = iterator.index;
                worst_distance = distance;
            }
        } else if m.bool(field(p::ENEMY)) == enemy {
            required_prop_count = required_prop_count.wrapping_add(1);
            if replace && distance < worst_required_distance {
                worst_required_prop_index = iterator.index;
                worst_required_distance = distance;
            }
        }
    }

    let mut prop_index = if worst_prop_index != NONE {
        worst_prop_index
    } else {
        let required_prop_maximum = if enemy { 6 } else { 4 };
        if worst_required_prop_index != NONE && required_prop_count >= required_prop_maximum {
            worst_required_prop_index
        } else {
            NONE
        }
    };

    if prop_index == NONE {
        prop_index = crate::data_array::datum_new(m, context.prop_data);
    } else {
        let prop = prop_get(m, context, prop_index);
        call_full(context, host, call::SWITCH_PROPS, prop_index, NONE);
        prop_remove(context, host, prop_index);
        let identifier = m.r16(prop);
        for offset in 0..p::SIZEOF as u32 {
            m.w8(prop + offset, 0);
        }
        m.w16(prop, identifier);
    }

    prop_add(context, host, unit_index, prop_index);
    prop_index
}
