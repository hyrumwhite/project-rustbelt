//! The encounters' update (encounters.c's `encounters_update`, every tick of
//! the AI's update): each active encounter in its turn (one tick in every
//! half second) counts its actors and their strength by squad and platoon,
//! keeps its enemy timers, stands down or begins its post-combat, respawns
//! its squads, runs out its squads' delay timers, tests its platoons'
//! rules, migrates its squads after its follow target and orders its actors
//! (standing down, defending, maneuvering).
//!
//! The encounters, squads, platoons and actors are the game's arrays,
//! worked on in place ([`WorldContext`]); activating the encounters,
//! standing down, the post-combat, spawning actors, the random seed, a
//! squad's timer running out, the follow targets, an object's origin,
//! moving an actor to another squad and the maneuvering stimulus are the
//! game's, through [`AiHost::call`] (encounters.c's `encounter_call`, shared
//! with the C). The rules' and the respawns' console lines (`ai_debug`'s
//! print switches) are the game's alone.

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext, datum_get, prop_get, tag_block_count, tag_block_element};
use crate::layout::{
    actor_datum as a, ai_globals, ai_values as v, encounter_datum as e, encounter_definition as ed,
    firing_position_definition as fp, platoon_datum as pl, platoon_definition as pd, platoon_rule as rule, prop_datum as p,
    scenario, squad_datum as sq, squad_definition as sd, unit_datum as u,
};

/// The encounters' calls (halo_sim.h's `_encounter_call_*`).
pub mod call {
    /// `encounters_test_activation()`.
    pub const TEST_ACTIVATION: i32 = 1;
    /// `encounter_stand_down(a)`.
    pub const STAND_DOWN: i32 = 2;
    /// `encounter_post_combat(a)`.
    pub const POST_COMBAT: i32 = 3;
    /// `game_team_is_ally(a, b)`, the result.
    pub const TEAM_IS_ALLY: i32 = 4;
    /// `encounter_spawn_actor(a, b)`, the result.
    pub const SPAWN_ACTOR: i32 = 5;
    /// `seed_random_range(global seed, a, b)`, the result.
    pub const RANDOM_RANGE: i32 = 6;
    /// `encounter_squad_timer_expire(a, b)`.
    pub const SQUAD_TIMER_EXPIRE: i32 = 7;
    /// How many units the encounter `a` follows (its players, its unit, or
    /// its AI's actors, eight at most; a follow unit that is gone is
    /// forgotten), the result.
    pub const FOLLOW_TARGET_COUNT: i32 = 8;
    /// The `b`th of the units the encounter `a` follows, the result.
    pub const FOLLOW_TARGET: i32 = 9;
    /// `object_get_origin(a)`, in `output[0..3]`.
    pub const OBJECT_ORIGIN: i32 = 10;
    /// `actor_change_encounter(actor, a, b)`.
    pub const ACTOR_CHANGE_ENCOUNTER: i32 = 11;
    /// `actor_stimulus_maneuvering(actor, a, b)`.
    pub const STIMULUS_MANEUVERING: i32 = 12;
}

const TICKS_PER_SECOND: i32 = 30;
const UPDATE_INTERVAL: i32 = TICKS_PER_SECOND / 2;
const ENEMY_RECENT_TICKS: i32 = TICKS_PER_SECOND * 2;
const ENEMY_MEMORY_TICKS: i32 = TICKS_PER_SECOND * 15;
const STRENGTH_FRACTION_EPSILON: f32 = 0.001;
const MAXIMUM_FOLLOW_TARGET_UNITS: usize = 8;
const NUMBER_OF_FIRING_POSITION_GROUP_INDICES: usize = 26;
const REAL_MAX: f32 = f32::MAX;
const REAL_MIN: f32 = -f32::MAX;
/// The local enums (encounters.c's and actors.c's).
const FOLLOW_TARGET_PLAYERS: i16 = 1;
const FOLLOW_TARGET_AI: i16 = 3;
const ACTOR_MODE_COMBAT: i16 = 3;
const ACTOR_COMBAT_STATUS_VISIBLE: i16 = 7;

/// `struct data_array`'s fields.
const DATA_SIZE: u32 = 0x22;
const DATA_COUNT: u32 = 0x2E;
const DATA_DATA: u32 = 0x34;

/// C's `MIN` and `MAX`.
fn min(a: f32, b: f32) -> f32 {
    if a > b { b } else { a }
}

fn max(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

/// `distance_squared3d(a, b)`.
fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

fn test_bit(bits: &[u32], index: usize) -> bool {
    bits[index / 32] & (1 << (index % 32)) != 0
}

fn set_bit(bits: &mut [u32], index: usize) {
    bits[index / 32] |= 1 << (index % 32);
}

/// The encounters' update on the game's arrays.
struct Encounters<'a, H: AiHost> {
    m: &'a dyn AiMemory,
    host: &'a H,
    context: &'a WorldContext,
}

impl<H: AiHost> Encounters<'_, H> {
    fn call(&self, kind: i32, actor: i32, a: i32, b: i32) -> AiCall {
        let mut call = AiCall { kind, actor, a, b, ..AiCall::default() };
        self.host.call(&mut call);
        call
    }

    fn initialized(&self) -> bool {
        self.m.bool(self.context.ai_globals + ai_globals::AI_INITIALIZED_FOR_MAP as u32)
    }

    /// `encounter_get`.
    fn encounter(&self, encounter_index: i32) -> u32 {
        datum_get(self.m, self.context.encounter_data, encounter_index).unwrap_or(0)
    }

    /// `actor_get`.
    fn actor(&self, actor_index: i32) -> u32 {
        datum_get(self.m, self.context.actor_data, actor_index).unwrap_or(0)
    }

    /// `encounter_get_squad`.
    fn squad(&self, encounter: u32, squad_index: i16) -> u32 {
        let absolute = squad_index.wrapping_add(self.m.i16(encounter + e::SQUAD_BASE as u32));
        self.context.squad_array + absolute as i32 as u32 * sq::SIZEOF as u32
    }

    /// `encounter_get_platoon`.
    fn platoon(&self, encounter: u32, platoon_index: i16) -> u32 {
        let absolute = platoon_index.wrapping_add(self.m.i16(encounter + e::PLATOON_BASE as u32));
        self.context.platoon_array + absolute as i32 as u32 * pl::SIZEOF as u32
    }

    /// The encounter's definition (the scenario's).
    fn definition(&self, encounter_index: i32) -> u32 {
        tag_block_element(
            self.m,
            self.context.scenario + scenario::AI_ENCOUNTERS as u32,
            encounter_index & 0xFFFF,
            ed::SIZEOF,
        )
    }

    /// The encounter's actors (`encounter_actor_iterator`): each index, read
    /// as the iterator reads it (the next one taken as an actor is
    /// returned).
    fn for_each_actor(&self, encounter_index: i32, mut f: impl FnMut(i32, u32)) {
        if !self.initialized() {
            return;
        }
        let mut next_index = self.m.i32(self.encounter(encounter_index) + e::FIRST_ACTOR_INDEX as u32);
        while next_index != NONE {
            let actor_index = next_index;
            let actor = self.actor(actor_index);
            next_index = self.m.i32(actor + a::META_NEXT_ACTOR_INDEX as u32);
            f(actor_index, actor);
        }
    }

    /// The encounters (`encounter_iterator`, all or the active ones): each
    /// index, read as `data_iterator_next` reads them.
    fn for_each_encounter(&self, active_only: bool, mut f: impl FnMut(i32, u32)) {
        if !self.initialized() {
            return;
        }
        let m = self.m;
        let data = self.context.encounter_data;
        let mut absolute_index = 0i16;
        loop {
            let size = m.i16(data + DATA_SIZE) as i32;
            if absolute_index >= m.i16(data + DATA_COUNT) {
                break;
            }
            let header = m.r32(data + DATA_DATA).wrapping_add((size * absolute_index as i32) as u32);
            let identifier = m.i16(header);
            let datum_index = ((identifier as i32) << 16) | absolute_index as i32;
            absolute_index += 1;
            if identifier != 0 && (!active_only || m.bool(header + e::ACTIVE as u32)) {
                f(datum_index, header);
            }
        }
    }

    /// actors.c's `actor_in_combat`.
    fn actor_in_combat(&self, actor: u32) -> bool {
        let m = self.m;
        m.i16(actor + a::STATE_MODE as u32) == ACTOR_MODE_COMBAT
            && m.i16(actor + a::STATE_COMBAT_STATUS as u32) > m.i16(actor + a::STATE_ARTIFICIAL_COMBAT_STATUS as u32)
    }

    /// actors.c's `actor_is_fighting`.
    fn actor_is_fighting(&self, actor: u32) -> bool {
        let m = self.m;
        let mut result = m.i16(actor + a::STATE_COMBAT_STATUS as u32) >= ACTOR_COMBAT_STATUS_VISIBLE;
        if result
            && m.i16(actor + a::STATE_ACTION as u32) == v::_ACTOR_ACTION_FLEE as i16
            && m.i16(actor + a::STATE_FLEE_PANIC_TYPE as u32) > 0
        {
            result = false;
        }
        result
    }

    /// `encounter_update_status`.
    fn update_status(&self, encounter_index: i32) {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let at = |offset: usize| encounter + offset as u32;
        let step = self.context.step_context();
        let mut real_enemy_targeted = false;
        let mut post_combat_behavior_pending = false;
        let mut had_visible_enemy = false;
        let mut been_in_combat = false;

        m.set_bool(at(e::ENEMY_ALIVE), false);
        m.set_bool(at(e::ENEMY_VISIBLE), false);
        m.set_i16(at(e::CURRENT_FIGHTING_COUNT), 0);
        m.set_i16(at(e::CURRENT_IN_COMBAT_COUNT), 0);
        m.set_i16(at(e::CURRENT_SWARM_COUNT), 0);
        m.set_i16(at(e::CURRENT_COUNT), 0);
        m.set_f32(at(e::CURRENT_STRENGTH_FRACTION), 0.0);
        for i in 0..m.i16(at(e::SQUAD_COUNT)) {
            let squad = self.squad(encounter, i);
            m.set_i16(squad + sq::CURRENT_SWARM_COUNT as u32, 0);
            m.set_i16(squad + sq::CURRENT_COUNT as u32, 0);
            m.set_f32(squad + sq::CURRENT_STRENGTH_FRACTION as u32, 0.0);
        }
        for i in 0..m.i16(at(e::PLATOON_COUNT)) {
            let platoon = self.platoon(encounter, i);
            m.set_i16(platoon + pl::CURRENT_SWARM_COUNT as u32, 0);
            m.set_i16(platoon + pl::CURRENT_COUNT as u32, 0);
            m.set_f32(platoon + pl::CURRENT_STRENGTH_FRACTION as u32, 0.0);
        }

        let add_i16 = |address: u32, value: i32| m.set_i16(address, (m.i16(address) as i32 + value) as i16);
        let add_f32 = |address: u32, value: f32| m.set_f32(address, m.f32(address) + value);
        self.for_each_actor(encounter_index, |_, actor| {
            let actor_at = |offset: usize| actor + offset as u32;
            let squad = self.squad(encounter, m.i16(actor_at(a::META_SQUAD_INDEX)));
            let unit_index = m.i32(actor_at(a::META_UNIT_INDEX));
            let (strength, body_count) = if unit_index != NONE {
                let unit = crate::ai_actor::object_get(m, &step, unit_index).unwrap_or(0);
                (m.f32(unit + u::OBJECT_BODY_VITALITY as u32), 1i32)
            } else {
                let count = m.i16(actor_at(a::META_SWARM_UNIT_COUNT));
                (count as f32 / m.i16(actor_at(a::META_SWARM_ORIGINAL_UNIT_COUNT)) as f32, count as i32)
            };
            let swarm = m.r8(actor_at(a::META_SWARM)) as i32;
            let platoon_index = m.i16(actor_at(a::META_PLATOON_INDEX));
            if platoon_index != NONE as i16 {
                let platoon = self.platoon(encounter, platoon_index);
                add_i16(platoon + pl::CURRENT_COUNT as u32, body_count);
                add_i16(platoon + pl::CURRENT_SWARM_COUNT as u32, swarm * body_count);
                add_f32(platoon + pl::CURRENT_STRENGTH_FRACTION as u32, strength);
            }
            add_i16(squad + sq::CURRENT_COUNT as u32, body_count);
            add_i16(squad + sq::CURRENT_SWARM_COUNT as u32, swarm * body_count);
            add_f32(squad + sq::CURRENT_STRENGTH_FRACTION as u32, strength);
            add_i16(at(e::CURRENT_COUNT), body_count);
            add_i16(at(e::CURRENT_SWARM_COUNT), swarm * body_count);
            add_i16(at(e::CURRENT_IN_COMBAT_COUNT), self.actor_in_combat(actor) as i32 * body_count);
            add_i16(at(e::CURRENT_FIGHTING_COUNT), self.actor_is_fighting(actor) as i32 * body_count);
            add_f32(at(e::CURRENT_STRENGTH_FRACTION), strength);

            let target_prop_index = m.i32(actor_at(a::TARGET_TARGET_PROP_INDEX));
            if target_prop_index != NONE {
                let prop = prop_get(m, &step, target_prop_index);
                m.set_bool(at(e::ENEMY_TARGET), true);
                let ally = self.call(
                    call::TEAM_IS_ALLY,
                    NONE,
                    m.i16(actor_at(a::META_TEAM_INDEX)) as i32,
                    m.i16(prop + p::TEAM_INDEX as u32) as i32,
                );
                if ally.result == 0 {
                    real_enemy_targeted = true;
                }
                if m.bool(actor_at(a::STATE_HAD_VISIBLE_ENEMY)) {
                    had_visible_enemy = true;
                }
                if m.bool(actor_at(a::STATE_BEEN_IN_COMBAT)) {
                    been_in_combat = true;
                }
                let prop_state = m.i16(prop + p::STATE as u32);
                if m.i16(actor_at(a::STATE_COMBAT_STATUS)) >= ACTOR_COMBAT_STATUS_VISIBLE {
                    m.set_bool(at(e::ENEMY_VISIBLE), true);
                    m.set_bool(at(e::ENEMY_ALIVE), true);
                } else if prop_state >= v::_PROP_STATE_BECOMING_UNACKNOWLEDGED as i16
                    && prop_state <= v::_PROP_STATE_ACKNOWLEDGED as i16
                {
                    if !m.bool(prop + p::DEAD as u32) {
                        m.set_bool(at(e::ENEMY_ALIVE), true);
                    }
                } else {
                    let unit = crate::ai_actor::object_get(m, &step, m.i32(prop + p::UNIT_INDEX as u32)).unwrap_or(0);
                    if m.r16(unit + u::OBJECT_DAMAGE_FLAGS as u32) & (1 << v::_OBJECT_DEAD_BIT) == 0 {
                        m.set_bool(at(e::ENEMY_ALIVE), true);
                    }
                }
            }

            if m.i16(actor_at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) > v::_ACTOR_POSTCOMBAT_NONE as i16 {
                post_combat_behavior_pending = true;
            }
        });

        if real_enemy_targeted {
            m.set_bool(at(e::ENEMY_TRAITOR), false);
        }

        let visible_timer = m.i32(at(e::ENEMY_VISIBLE_TIMER));
        let alive_timer = m.i32(at(e::ENEMY_ALIVE_TIMER));
        let stay_active = m.bool(at(e::ENEMY_VISIBLE))
            || (visible_timer != NONE && visible_timer < ENEMY_RECENT_TICKS)
            || ((m.bool(at(e::ENEMY_ALIVE)) || (alive_timer != NONE && alive_timer < ENEMY_RECENT_TICKS))
                && visible_timer != NONE
                && visible_timer < ENEMY_MEMORY_TICKS);
        if stay_active {
            m.set_bool(at(e::STAND_DOWN), false);
            m.set_bool(at(e::POST_COMBAT), false);
        } else if m.bool(at(e::STAND_DOWN)) {
            m.set_bool(at(e::POST_COMBAT), false);
            m.set_i16(at(e::PREBATTLE_LIVING_COUNT), m.i16(at(e::CURRENT_COUNT)));
            m.w32(at(e::CORPSE_IGNORE_TIME), self.context.game_time as u32);
            m.set_i16(at(e::ENEMIES_DEFEATED), 0);
            if !m.bool(at(e::ENEMY_TARGET)) {
                m.w32(at(e::ENEMY_VISIBLE_TIMER), NONE as u32);
                m.w32(at(e::ENEMY_ALIVE_TIMER), NONE as u32);
            }
        } else if m.bool(at(e::POST_COMBAT)) {
            m.set_bool(at(e::POST_COMBAT_DELAY), !post_combat_behavior_pending);
            if m.i16(at(e::POST_COMBAT_DELAY_TIMER)) == 0 {
                self.call(call::STAND_DOWN, NONE, encounter_index, 0);
            }
        } else if had_visible_enemy && been_in_combat {
            self.call(call::POST_COMBAT, NONE, encounter_index, 0);
        } else {
            self.call(call::STAND_DOWN, NONE, encounter_index, 0);
        }

        let original_count = m.i16(at(e::ORIGINAL_COUNT));
        if original_count > 0 {
            let fraction = m.f32(at(e::CURRENT_STRENGTH_FRACTION)) / original_count as f32 - STRENGTH_FRACTION_EPSILON;
            m.set_f32(at(e::CURRENT_STRENGTH_FRACTION), max(0.0, fraction));
        }
        for i in 0..m.i16(at(e::SQUAD_COUNT)) {
            let squad = self.squad(encounter, i);
            let fraction = m.f32(squad + sq::CURRENT_STRENGTH_FRACTION as u32)
                / m.i16(squad + sq::ORIGINAL_COUNT as u32) as f32
                - STRENGTH_FRACTION_EPSILON;
            m.set_f32(squad + sq::CURRENT_STRENGTH_FRACTION as u32, max(0.0, fraction));
        }
        for i in 0..m.i16(at(e::PLATOON_COUNT)) {
            let platoon = self.platoon(encounter, i);
            let fraction = m.f32(platoon + pl::CURRENT_STRENGTH_FRACTION as u32)
                / m.i16(platoon + pl::ORIGINAL_COUNT as u32) as f32
                - STRENGTH_FRACTION_EPSILON;
            m.set_f32(platoon + pl::CURRENT_STRENGTH_FRACTION as u32, max(0.0, fraction));
        }
        m.set_bool(at(e::STATUS_DIRTY), false);
    }

    /// `encounters_update_dirty_status`.
    fn update_dirty_status(&self) {
        self.for_each_encounter(false, |encounter_index, encounter| {
            if self.m.bool(encounter + e::STATUS_DIRTY as u32) {
                self.update_status(encounter_index);
            }
        });
    }

    /// `encounter_update_timers`.
    fn update_timers(&self, encounter_index: i32) {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let at = |offset: usize| encounter + offset as u32;
        for (flag, timer) in [(e::ENEMY_VISIBLE, e::ENEMY_VISIBLE_TIMER), (e::ENEMY_ALIVE, e::ENEMY_ALIVE_TIMER)] {
            if m.bool(at(flag)) {
                m.w32(at(timer), 0);
            } else if m.i32(at(timer)) != NONE {
                m.w32(at(timer), m.i32(at(timer)).wrapping_add(UPDATE_INTERVAL) as u32);
            }
        }
        if m.bool(at(e::POST_COMBAT)) && m.bool(at(e::POST_COMBAT_DELAY)) {
            let timer = m.i16(at(e::POST_COMBAT_DELAY_TIMER));
            m.set_i16(
                at(e::POST_COMBAT_DELAY_TIMER),
                if timer as i32 > UPDATE_INTERVAL { (timer as i32 - UPDATE_INTERVAL) as i16 } else { 0 },
            );
        }
    }

    /// `encounter_test_rule`.
    fn test_rule(&self, encounter_index: i32, rule_address: u32) -> bool {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let platoon_index = m.i16(rule_address + rule::PLATOON_INDEX as u32);
        let (original_count, current_strength_fraction, current_count) =
            if platoon_index >= 0 && platoon_index < m.i16(encounter + e::PLATOON_COUNT as u32) {
                let platoon = self.platoon(encounter, platoon_index);
                (
                    m.i16(platoon + pl::ORIGINAL_COUNT as u32) as i32,
                    m.f32(platoon + pl::CURRENT_STRENGTH_FRACTION as u32),
                    m.i16(platoon + pl::CURRENT_COUNT as u32) as i32,
                )
            } else {
                (
                    m.i16(encounter + e::ORIGINAL_COUNT as u32) as i32,
                    m.f32(encounter + e::CURRENT_STRENGTH_FRACTION as u32),
                    m.i16(encounter + e::CURRENT_COUNT as u32) as i32,
                )
            };
        if original_count <= 0 {
            return false;
        }
        match m.i16(rule_address + rule::RULE_TYPE as u32) as usize {
            v::_PLATOON_RULE_75_STRENGTH => current_strength_fraction < 0.75,
            v::_PLATOON_RULE_50_STRENGTH => current_strength_fraction < 0.5,
            v::_PLATOON_RULE_25_STRENGTH => current_strength_fraction < 0.25,
            v::_PLATOON_RULE_ANYBODY_DEAD => current_count < original_count,
            v::_PLATOON_RULE_25_DEAD => current_count * 4 / 3 <= original_count,
            v::_PLATOON_RULE_50_DEAD => current_count * 2 <= original_count,
            v::_PLATOON_RULE_75_DEAD => current_count * 4 <= original_count,
            v::_PLATOON_RULE_ALL_BUT_ONE_DEAD => current_count <= 1,
            v::_PLATOON_RULE_ALL_DEAD => current_count == 0,
            _ => false,
        }
    }

    /// `encounter_update_respawn`.
    fn update_respawn(&self, encounter_index: i32) {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let at = |offset: usize| encounter + offset as u32;
        if !m.bool(at(e::RESPAWN_ENABLED)) {
            return;
        }
        let delay = m.i16(at(e::RESPAWN_DELAY_TICKS));
        if delay as i32 > UPDATE_INTERVAL {
            m.set_i16(at(e::RESPAWN_DELAY_TICKS), (delay as i32 - UPDATE_INTERVAL) as i16);
            return;
        }

        let definition = self.definition(encounter_index);
        let squads = definition + ed::SQUADS as u32;
        m.set_i16(at(e::RESPAWN_DELAY_TICKS), 0);
        let mut respawn_squad_count = 0i16;
        let mut respawn_squads = [0u32; 2];

        for squad_index in 0..tag_block_count(m, squads) as i16 {
            let squad = self.squad(encounter, squad_index);
            let squad_definition = tag_block_element(m, squads, squad_index as i32, sd::SIZEOF);
            let left = |m: &dyn AiMemory| m.i16(squad + sq::RESPAWN_ACTORS_LEFT as u32);
            let current = |m: &dyn AiMemory| m.i16(squad + sq::CURRENT_COUNT as u32);
            while left(m) > 0 && current(m) < m.i16(squad_definition + sd::RESPAWN_MIN_ACTORS as u32) {
                if self.call(call::SPAWN_ACTOR, NONE, encounter_index, squad_index as i32).result == 0 {
                    break;
                }
            }

            if left(m) > 0 && current(m) < m.i16(squad_definition + sd::RESPAWN_MAX_ACTORS as u32) {
                let squad_delay = m.i16(squad + sq::RESPAWN_DELAY_TICKS as u32);
                if squad_delay as i32 > UPDATE_INTERVAL {
                    m.set_i16(squad + sq::RESPAWN_DELAY_TICKS as u32, (squad_delay as i32 - UPDATE_INTERVAL) as i16);
                } else {
                    respawn_squad_count += 1;
                    m.set_i16(squad + sq::RESPAWN_DELAY_TICKS as u32, 0);
                    set_bit(&mut respawn_squads, squad_index as usize);
                }
            }
        }

        if respawn_squad_count > 0 && m.i16(at(e::RESPAWN_DELAY_TICKS)) == 0 {
            let mut respawn_squad_index =
                self.call(call::RANDOM_RANGE, NONE, 0, respawn_squad_count as i32).result as i16;
            for squad_index in 0..m.i16(at(e::SQUAD_COUNT)) {
                if test_bit(&respawn_squads, squad_index as usize) {
                    if respawn_squad_index > 0 {
                        respawn_squad_index -= 1;
                    } else if self.call(call::SPAWN_ACTOR, NONE, encounter_index, squad_index as i32).result != 0 {
                        return;
                    }
                }
            }
        }
    }

    /// `encounter_update_squads`.
    fn update_squads(&self, encounter_index: i32) {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let squads = self.definition(encounter_index) + ed::SQUADS as u32;
        for squad_index in 0..m.i16(encounter + e::SQUAD_COUNT as u32) {
            let squad = self.squad(encounter, squad_index);
            let squad_definition = tag_block_element(m, squads, squad_index as i32, sd::SIZEOF);
            let flags = m.r32(squad_definition + sd::FLAGS as u32);
            let delay_timer = m.i16(squad + sq::DELAY_TIMER as u32);
            if delay_timer > 0 && flags & (1 << v::_SQUAD_DELAY_FOREVER_BIT) == 0 {
                if m.bool(squad + sq::DELAY_TIMER_STARTED as u32) {
                    if delay_timer as i32 > UPDATE_INTERVAL {
                        m.set_i16(squad + sq::DELAY_TIMER as u32, (delay_timer as i32 - UPDATE_INTERVAL) as i16);
                    } else {
                        self.call(call::SQUAD_TIMER_EXPIRE, NONE, encounter_index, squad_index as i32);
                    }
                } else {
                    m.set_bool(
                        squad + sq::DELAY_TIMER_STARTED as u32,
                        flags & (1 << v::_SQUAD_TIMER_STARTS_IMMEDIATELY_BIT) != 0
                            || m.i16(encounter + e::CURRENT_IN_COMBAT_COUNT as u32) > 0,
                    );
                }
            }
        }
    }

    /// `encounter_update_platoons`.
    fn update_platoons(&self, encounter_index: i32) {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let platoons = self.definition(encounter_index) + ed::PLATOONS as u32;
        for platoon_index in 0..m.i16(encounter + e::PLATOON_COUNT as u32) {
            let platoon = self.platoon(encounter, platoon_index);
            if m.i16(platoon + pl::CURRENT_COUNT as u32) <= 0 {
                continue;
            }
            let platoon_definition = tag_block_element(m, platoons, platoon_index as i32, pd::SIZEOF);
            if !m.bool(platoon + pl::MANEUVERING as u32) {
                let maneuvering = self.test_rule(encounter_index, platoon_definition + pd::MANEUVERING_RULE as u32);
                m.set_bool(platoon + pl::MANEUVERING as u32, maneuvering);
            }
            if m.bool(platoon + pl::MANEUVER_DISABLE as u32) || !m.bool(platoon + pl::MANEUVERING as u32) {
                let defending =
                    m.r32(platoon_definition + pd::FLAGS as u32) & (1 << v::_PLATOON_INITIALLY_DEFENDING_BIT) == 0;
                if m.bool(platoon + pl::DEFENDING as u32) != defending
                    && self.test_rule(encounter_index, platoon_definition + pd::ATTACKING_DEFENDING_RULE as u32)
                {
                    m.set_bool(platoon + pl::DEFENDING as u32, defending);
                }
            }
        }
    }

    /// `encounter_update_follow`.
    fn update_follow(&self, encounter_index: i32) {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let at = |offset: usize| encounter + offset as u32;
        let definition = self.definition(encounter_index);
        let squads = definition + ed::SQUADS as u32;
        let platoons = definition + ed::PLATOONS as u32;
        let follow_type = m.i16(at(e::FOLLOW_TARGET_TYPE));
        let target_count = if (FOLLOW_TARGET_PLAYERS..=FOLLOW_TARGET_AI).contains(&follow_type) {
            (self.call(call::FOLLOW_TARGET_COUNT, NONE, encounter_index, 0).result as usize).min(MAXIMUM_FOLLOW_TARGET_UNITS)
        } else {
            0
        };
        if target_count == 0 {
            return;
        }
        let targets: Vec<i32> =
            (0..target_count).map(|index| self.call(call::FOLLOW_TARGET, NONE, encounter_index, index as i32).result).collect();

        let mut present_squads = [0u32; 2];
        let mut migrating_squads = [0u32; 2];
        let mut living_squads = [0u32; 2];
        let mut squad_firing_position_groups = [0u32; 64];
        let mut firing_position_groups = 0u32;
        let mut living_count = 0i16;

        for squad_index in 0..m.i16(at(e::SQUAD_COUNT)) {
            let squad_definition = tag_block_element(m, squads, squad_index as i32, sd::SIZEOF);
            if m.r32(squad_definition + sd::FLAGS as u32) & (1 << v::_SQUAD_AUTOMATIC_MIGRATION_BIT) == 0 {
                continue;
            }
            let squad = self.squad(encounter, squad_index);
            set_bit(&mut present_squads, squad_index as usize);
            living_count = living_count.wrapping_add(m.i16(squad + sq::CURRENT_COUNT as u32));
            if !m.bool(squad + sq::AUTOMATIC_MIGRATION_TARGET as u32) {
                continue;
            }
            set_bit(&mut migrating_squads, squad_index as usize);
            let platoon_index = m.i16(squad_definition + sd::PLATOON_INDEX as u32);
            let defending = if platoon_index >= 0 && (platoon_index as i32) < tag_block_count(m, platoons) {
                m.bool(self.platoon(encounter, platoon_index) + pl::DEFENDING as u32)
            } else {
                false
            };
            let groups_range = if defending {
                v::_FIRING_POSITION_GROUP_DEFENDING..=v::_FIRING_POSITION_GROUP_DEFENDING_GUARD
            } else {
                v::_FIRING_POSITION_GROUP_ATTACKING..=v::_FIRING_POSITION_GROUP_ATTACKING_GUARD
            };
            let mut groups = 0u32;
            for group_index in groups_range {
                groups |= m.r32(squad_definition + sd::FIRING_POSITION_GROUPS as u32 + 4 * group_index as u32);
            }
            firing_position_groups |= groups;
            squad_firing_position_groups[squad_index as usize] = groups;
            if m.i16(squad + sq::CURRENT_COUNT as u32) > 0 {
                set_bit(&mut living_squads, squad_index as usize);
            }
        }

        if living_count <= 0 || firing_position_groups == 0 {
            return;
        }

        let origin = |object_index: i32| self.call(call::OBJECT_ORIGIN, NONE, object_index, 0).vector();
        let mut follow_unit_index = NONE;
        let mut follow_position = [0.0f32; 3];
        if target_count == 1 {
            follow_unit_index = targets[0];
            follow_position = origin(follow_unit_index);
        } else {
            let mut distances_squared = [REAL_MAX; MAXIMUM_FOLLOW_TARGET_UNITS];
            let positions: Vec<[f32; 3]> = targets.iter().map(|&unit| origin(unit)).collect();
            self.for_each_actor(encounter_index, |_, actor| {
                if test_bit(&present_squads, m.i16(actor + a::META_SQUAD_INDEX as u32) as usize) {
                    let body = m.vector(actor + a::INPUT_BODY_POSITION as u32);
                    for (target_index, position) in positions.iter().enumerate() {
                        let distance_squared = distance_squared(body, *position);
                        distances_squared[target_index] = min(distances_squared[target_index], distance_squared);
                    }
                }
            });
            for target_index in 0..target_count {
                if distances_squared[target_index] < REAL_MAX {
                    follow_unit_index = targets[target_index];
                    follow_position = positions[target_index];
                }
            }
        }

        if follow_unit_index == NONE {
            return;
        }
        let mut group_distances_squared = [REAL_MAX; NUMBER_OF_FIRING_POSITION_GROUP_INDICES];
        let firing_positions = definition + ed::FIRING_POSITIONS as u32;
        for firing_position_index in 0..tag_block_count(m, firing_positions) as i16 {
            let firing_position = tag_block_element(m, firing_positions, firing_position_index as i32, fp::SIZEOF);
            let group_index = m.i16(firing_position + fp::GROUP_INDEX as u32);
            if (0..NUMBER_OF_FIRING_POSITION_GROUP_INDICES as i16).contains(&group_index)
                && firing_position_groups & (1 << group_index) != 0
            {
                let distance_squared = distance_squared(m.vector(firing_position + fp::POSITION as u32), follow_position);
                let group = group_index as usize;
                group_distances_squared[group] = min(group_distances_squared[group], distance_squared);
            }
        }

        let mut best_distance_squared = REAL_MAX;
        let mut current_distance_squared = REAL_MIN;
        let mut best_squad_index = NONE as i16;
        let mut current_squad_index = NONE as i16;
        for candidate in 0..m.i16(at(e::SQUAD_COUNT)) {
            if !test_bit(&migrating_squads, candidate as usize) {
                continue;
            }
            let mut squad_distance_squared = REAL_MAX;
            for (group_index, &distance) in group_distances_squared.iter().enumerate() {
                if squad_firing_position_groups[candidate as usize] & (1 << group_index) != 0 {
                    squad_distance_squared = min(squad_distance_squared, distance);
                }
            }
            if squad_distance_squared < best_distance_squared {
                best_distance_squared = squad_distance_squared;
                best_squad_index = candidate;
            }
            if test_bit(&living_squads, candidate as usize) && squad_distance_squared > current_distance_squared {
                current_distance_squared = squad_distance_squared;
                current_squad_index = candidate;
            }
        }

        if best_squad_index == NONE as i16 {
            return;
        }
        let mut migrate = true;
        if current_squad_index != NONE as i16 {
            // square_root: the C's double sqrt
            let best_distance = best_distance_squared.sqrt();
            let current_distance = current_distance_squared.sqrt();
            let follow_distance = m.f32(at(e::FOLLOW_TARGET_DISTANCE));
            let tolerance = if follow_distance > 0.0 { follow_distance } else { 2.0 };
            migrate = best_distance < current_distance - tolerance;
        }
        if migrate {
            self.for_each_actor(encounter_index, |actor_index, actor| {
                let squad_index = m.i16(actor + a::META_SQUAD_INDEX as u32);
                if test_bit(&present_squads, squad_index as usize) && squad_index != best_squad_index {
                    self.call(call::ACTOR_CHANGE_ENCOUNTER, actor_index, encounter_index, best_squad_index as i32);
                }
            });
        }
    }

    /// `encounter_control_actors`.
    fn control_actors(&self, encounter_index: i32) {
        let m = self.m;
        let encounter = self.encounter(encounter_index);
        let at = |offset: usize| encounter + offset as u32;
        let definition = self.definition(encounter_index);
        let squads = definition + ed::SQUADS as u32;
        let platoons = definition + ed::PLATOONS as u32;
        self.for_each_actor(encounter_index, |actor_index, actor| {
            let actor_at = |offset: usize| actor + offset as u32;
            let mut defending = false;
            let mut maneuvering = false;
            m.set_bool(actor_at(a::EXTERNAL_ORDERS_STAND_DOWN), m.bool(at(e::STAND_DOWN)));
            m.set_bool(actor_at(a::EXTERNAL_ORDERS_PLAYFIGHTING), m.bool(at(e::PLAYFIGHTING)));
            if !m.bool(at(e::POST_COMBAT)) {
                m.set_i16(actor_at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE), v::_ACTOR_POSTCOMBAT_NONE as i16);
                m.w32(actor_at(a::EXTERNAL_ORDERS_POSTCOMBAT_PROP_INDEX), NONE as u32);
            }
            let platoon_index = m.i16(actor_at(a::META_PLATOON_INDEX));
            if platoon_index != NONE as i16 {
                let platoon = self.platoon(encounter, platoon_index);
                defending = m.bool(platoon + pl::DEFENDING as u32);
                maneuvering = m.bool(platoon + pl::MANEUVERING as u32) && !m.bool(platoon + pl::MANEUVER_DISABLE as u32);
            }
            m.set_bool(actor_at(a::EXTERNAL_ORDERS_DEFENDING), defending);

            if maneuvering {
                let squad_definition =
                    tag_block_element(m, squads, m.i16(actor_at(a::META_SQUAD_INDEX)) as i32, sd::SIZEOF);
                let platoon_definition = tag_block_element(m, platoons, platoon_index as i32, pd::SIZEOF);
                let squad_index = m.i16(squad_definition + sd::MANEUVER_SQUAD_INDEX as u32);
                if squad_index >= 0 && (squad_index as i32) < tag_block_count(m, squads) {
                    let flags = m.r32(platoon_definition + pd::FLAGS as u32);
                    self.call(call::ACTOR_CHANGE_ENCOUNTER, actor_index, encounter_index, squad_index as i32);
                    self.call(
                        call::STIMULUS_MANEUVERING,
                        actor_index,
                        (flags & (1 << v::_PLATOON_ADVANCING_MANEUVER_BIT) != 0) as i32,
                        (flags & (1 << v::_PLATOON_FLEE_UPON_MANEUVER_BIT) != 0) as i32,
                    );
                }
            }
        });
        self.update_dirty_status();
    }
}

/// `encounters_update`.
pub fn encounters_update(context: &WorldContext, host: &impl AiHost) {
    let encounters = Encounters { m: host.memory(), host, context };
    let m = encounters.m;
    let time = context.game_time;
    if time % TICKS_PER_SECOND == 0 {
        encounters.update_dirty_status();
        encounters.call(call::TEST_ACTIVATION, NONE, 0, 0);
    }
    let phase = time % UPDATE_INTERVAL;
    encounters.for_each_encounter(true, |encounter_index, _| {
        let meter = context.encounters_updated_meter;
        m.set_i16(meter, m.i16(meter).wrapping_add(1));
        if (encounter_index & 0xFFFF) as i16 as i32 % UPDATE_INTERVAL == phase {
            encounters.update_status(encounter_index);
            encounters.update_timers(encounter_index);
            encounters.update_respawn(encounter_index);
            encounters.update_squads(encounter_index);
            encounters.update_platoons(encounter_index);
            encounters.update_follow(encounter_index);
            encounters.control_actors(encounter_index);
        }
    });
}
