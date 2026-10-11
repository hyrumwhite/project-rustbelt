//! The actor types' decisions: actor_types.c's `actor_type_decide_action`
//! and each actor_type_*.c's `*_decide_action`, which choose an actor's
//! action from its situation by actions.c's transition handlers.
//!
//! The handlers ported to Rust ([`crate::actor_transitions`]) run here; the
//! rest are still the game's: each goes through [`AiHost::call`]
//! (actor_types.c's `actor_type_call`, shared with the C).

use crate::actions::flee_blind_panic;
use crate::actor_transitions;
use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE};
use crate::layout::{actor_datum as a, actor_definition as d};

/// The types' calls (halo_sim.h's `_actor_type_call_*`): actions.c's
/// handlers, each with the actor, its arguments in `a`, `b` and `input[0]`,
/// and what it returns in the result.
pub mod call {
    const BASE: i32 = 0x1000;
    pub const INITIAL_ACTION: i32 = BASE | 1;
    pub const PENDING_COMMAND_LIST: i32 = BASE | 2;
    /// `a` the surprise level.
    pub const SURPRISE: i32 = BASE | 3;
    pub const DENY_TRANSITION: i32 = BASE | 4;
    pub const PANIC_FROM_SURPRISE: i32 = BASE | 5;
    pub const PANIC_FROM_DAMAGE: i32 = BASE | 6;
    pub const PANIC_FROM_ATTACHED_PROJECTILES: i32 = BASE | 7;
    pub const PANIC_FROM_ATTACHED_MELEE_ATTACKERS: i32 = BASE | 8;
    pub const PANIC_FROM_BURNING_TO_DEATH: i32 = BASE | 9;
    /// `a` the minimum panic type, `b` whether to communicate the failure,
    /// `input[0]` the forced panic type.
    pub const PANIC_TRANSITION: i32 = BASE | 10;
    pub const COMBAT_TRANSITION: i32 = BASE | 11;
    pub const VEHICLE_ENTRY: i32 = BASE | 12;
    pub const VEHICLE_EXIT: i32 = BASE | 13;
    pub const GRENADE_THROWING: i32 = BASE | 14;
    pub const DANGER_AVOIDANCE: i32 = BASE | 15;
    /// `a` whether initiative is allowed, `b` whether to force a decision.
    pub const COMBAT_STATUS: i32 = BASE | 16;
    pub const COMBAT_FAILURE: i32 = BASE | 17;
    pub const EVASION: i32 = BASE | 18;
    /// `a` the guard's investigate threshold, `b` the cower's.
    pub const CAN_STOP_GUARDING: i32 = BASE | 19;
    pub const DONE_FLEEING: i32 = BASE | 20;
    pub const CONSIDER_GRENADE: i32 = BASE | 21;
    pub const EXIT_PURSUIT: i32 = BASE | 22;
    pub const CAN_STOP_CONVERSING: i32 = BASE | 23;
    pub const BERSERKING_FROM_ATTACKING_MODE: i32 = BASE | 24;
    pub const BERSERKING_FROM_DAMAGE: i32 = BASE | 25;
    pub const BERSERKING_FROM_PROXIMITY: i32 = BASE | 26;
    /// `a` the berserk level.
    pub const BERSERK_TRANSITION: i32 = BASE | 27;
    pub const BERSERKING_FROM_ATTACHED_PROJECTILES: i32 = BASE | 28;
    /// `a` whether defensive cover is allowed, `b` whether to force the flee
    /// transition.
    pub const ACTIVE_COVER_SEEKING: i32 = BASE | 29;
}

/// The actor types (actor_types.h's `_actor_*`).
mod actor_type {
    pub const ELITE: i16 = 0;
    pub const JACKAL: i16 = 1;
    pub const HUNTER: i16 = 3;
    pub const ENGINEER: i16 = 4;
    pub const ASSASSIN: i16 = 5;
    pub const PLAYER: i16 = 6;
    pub const MARINE: i16 = 7;
    pub const CREW: i16 = 8;
    pub const COMBAT_FORM: i16 = 9;
    pub const INFECTION_FORM: i16 = 10;
    pub const CARRIER_FORM: i16 = 11;
    pub const MONITOR: i16 = 12;
    pub const SENTINEL: i16 = 13;
    pub const MOUNTED_WEAPON: i16 = 15;
}

/// The actions (actions.h's `_actor_action_*`).
mod action {
    pub const FIGHT: i16 = 3;
    pub const FLEE: i16 = 4;
    pub const UNCOVER: i16 = 5;
    pub const GUARD: i16 = 6;
    pub const SEARCH: i16 = 7;
    pub const WAIT: i16 = 8;
    pub const VEHICLE: i16 = 9;
    pub const CHARGE: i16 = 10;
    pub const OBEY: i16 = 11;
    pub const CONVERSE: i16 = 12;
    pub const AVOID: i16 = 13;
}

/// The decisions of an actor of one type: its handlers through the game.
struct Decide<'a, H: AiHost> {
    context: &'a AiStepContext,
    host: &'a H,
}

impl<H: AiHost> Decide<'_, H> {
    fn handle3(&self, kind: i32, a: i32, b: i32, c: i32) -> bool {
        if let Some(result) = actor_transitions::run(kind, a, b, c, self.context, self.host) {
            return result;
        }
        let mut call = AiCall { kind, actor: self.context.actor_index, a, b, ..AiCall::default() };
        call.input[0] = c as f32;
        self.host.call(&mut call);
        call.result != 0
    }

    fn handle2(&self, kind: i32, a: i32, b: i32) -> bool {
        self.handle3(kind, a, b, 0)
    }

    fn handle1(&self, kind: i32, a: i32) -> bool {
        self.handle3(kind, a, 0, 0)
    }

    fn handle(&self, kind: i32) -> bool {
        self.handle3(kind, 0, 0, 0)
    }

    fn at(&self, offset: usize) -> u32 {
        self.context.actor + offset as u32
    }

    fn action(&self) -> i16 {
        self.host.memory().i16(self.at(a::STATE_ACTION))
    }

    fn combat_status(&self, allow_initiative: bool, force_decision: bool) -> bool {
        self.handle2(call::COMBAT_STATUS, allow_initiative as i32, force_decision as i32)
    }

    fn can_stop_guarding(&self) -> bool {
        self.handle2(call::CAN_STOP_GUARDING, action::FIGHT as i32, action::GUARD as i32)
    }

    fn unable_to_flee(&self) -> bool {
        self.host.memory().bool(self.at(a::STATE_FLEE_UNABLE_TO_FLEE))
    }

    fn vehicle_entry_over(&self) -> bool {
        let m = self.host.memory();
        m.bool(self.at(a::STATE_VEHICLE_VEHICLE_ENTRY_DONE)) || m.bool(self.at(a::STATE_VEHICLE_VEHICLE_ENTRY_FAILED))
    }

    /// The combat status of an obeying actor (its initiative, and its list's
    /// end forcing a decision).
    fn obey(&self) {
        let m = self.host.memory();
        self.combat_status(m.bool(self.at(a::STATE_OBEY_INITIATIVE)), m.bool(self.at(a::STATE_OBEY_FINISHED)));
    }

    /// The combat status of a conversing actor.
    fn converse(&self) {
        let m = self.host.memory();
        let can_stop = self.handle(call::CAN_STOP_CONVERSING);
        let force = m.bool(self.at(a::STATE_CONVERSE_FAILED)) || m.i32(self.at(a::EXTERNAL_ORDERS_CONVERSATION_INDEX)) == NONE;
        self.combat_status(can_stop, force);
    }

    /// The combat status of an avoiding actor, once the danger's gone.
    fn avoid(&self) {
        if self.host.memory().i16(self.at(a::DANGER_ZONE_DANGER_TYPE)) == 0 {
            self.combat_status(true, true);
        }
    }

    /// Fighting or charging: the combat status, else its failure, else
    /// evasion.
    fn fight(&self) {
        if !self.combat_status(true, false) && !self.handle(call::COMBAT_FAILURE) {
            self.handle(call::EVASION);
        }
    }

    /// Fleeing: the combat status if it can't, else whether it's done.
    fn flee(&self) {
        if self.unable_to_flee() {
            self.combat_status(true, true);
        } else {
            self.handle(call::DONE_FLEEING);
        }
    }

    /// Uncovering, searching or waiting: the combat status, else the
    /// pursuit's exit.
    fn pursue(&self) {
        if !self.combat_status(true, false) {
            self.handle(call::EXIT_PURSUIT);
        }
    }

    /// The elite's and jackal's cower: it stays down while its shield is
    /// below the fraction to emerge at.
    fn cower(&self) {
        let m = self.host.memory();
        if m.bool(self.at(a::STATE_GUARD_COWER))
            && !m.bool(self.at(a::STATE_GUARD_COWER_PANICKED))
            && !m.bool(self.at(a::STATE_GUARD_COWER_FROM_RETREAT))
        {
            let fraction = if m.i16(self.at(a::STATE_COMBAT_STATUS)) >= 4 {
                d::DEFENSIVE_SHIELD_FRACTION_EMERGE_ATTACK
            } else {
                d::DEFENSIVE_SHIELD_FRACTION_EMERGE_PURSUE
            };
            let shield_fraction = m.f32(self.context.definition + fraction as u32);
            let cower = m.f32(self.at(a::INPUT_SHIELD_VITALITY)) < shield_fraction;
            m.set_bool(self.at(a::STATE_GUARD_COWER), cower);
            m.set_i16(self.at(a::STATE_GUARD_COWER_TICKS), if cower { 30 } else { 0 });
        }
    }

    /// The panics most types share: from surprise (when asked), damage,
    /// attached projectiles and melee attackers, and burning to death.
    fn panics(&self, from_surprise: bool) {
        if from_surprise {
            self.handle(call::PANIC_FROM_SURPRISE);
        }
        self.handle(call::PANIC_FROM_DAMAGE);
        self.handle(call::PANIC_FROM_ATTACHED_PROJECTILES);
        self.handle(call::PANIC_FROM_ATTACHED_MELEE_ATTACKERS);
        self.handle(call::PANIC_FROM_BURNING_TO_DEATH);
    }

    /// The decisions every type starts with: its initial action, a pending
    /// command list and (for most) surprise.
    fn begin(&self, surprise_level: Option<i32>) {
        self.handle(call::INITIAL_ACTION);
        self.handle(call::PENDING_COMMAND_LIST);
        if let Some(level) = surprise_level {
            self.handle1(call::SURPRISE, level);
        }
    }

    /// actor_type_grunt.c's `grunt_decide_action`.
    fn grunt(&self) {
        let m = self.host.memory();
        let nearby_elite = m.i8(self.at(a::SITUATION_AREA_FRIENDS_BY_TYPE) + actor_type::ELITE as u32) > 0;
        let nearby_elite_close = m.i8(self.at(a::SITUATION_CLOSE_FRIENDS_BY_TYPE) + actor_type::ELITE as u32) > 0;

        self.begin(Some(1));
        if !self.handle(call::DENY_TRANSITION) {
            self.panics(true);
            self.handle3(call::PANIC_TRANSITION, 1, nearby_elite as i32, 7);
            self.handle(call::COMBAT_TRANSITION);
            self.handle(call::VEHICLE_ENTRY);
            self.handle(call::VEHICLE_EXIT);
            self.handle(call::GRENADE_THROWING);
            self.handle(call::DANGER_AVOIDANCE);
        }

        match self.action() {
            action::FIGHT | action::CHARGE => self.fight(),
            action::GUARD => {
                let can_stop = self.handle2(call::CAN_STOP_GUARDING, action::FIGHT as i32, 6);
                self.combat_status(can_stop, false);
            }
            action::FLEE => {
                let panic_type = self.at(a::STATE_FLEE_PANIC_TYPE);
                if nearby_elite_close && m.i16(panic_type) > 0 && !flee_blind_panic(m.i16(panic_type)) {
                    m.set_bool(self.at(a::STATE_FLEE_DONE_FLEEING), true);
                }
                if self.unable_to_flee() {
                    self.combat_status(true, true);
                    return;
                }
                if self.handle(call::DONE_FLEEING) || m.i16(panic_type) != 0 || m.i16(self.at(a::STATE_COMBAT_STATUS)) < 5 {
                    return;
                }
                self.handle(call::CONSIDER_GRENADE);
            }
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::VEHICLE => {
                if self.vehicle_entry_over() {
                    self.combat_status(true, true);
                }
            }
            action::OBEY => self.obey(),
            action::CONVERSE => self.converse(),
            action::AVOID => self.avoid(),
            _ => {}
        }
    }

    /// actor_type_elite.c's `elite_decide_action`.
    fn elite(&self) {
        let m = self.host.memory();
        self.begin(Some(4));
        if !self.handle(call::DENY_TRANSITION) {
            self.handle(call::BERSERKING_FROM_ATTACKING_MODE);
            self.handle(call::BERSERKING_FROM_DAMAGE);
            self.handle(call::BERSERKING_FROM_PROXIMITY);
            self.handle(call::BERSERKING_FROM_ATTACHED_PROJECTILES);
            self.handle1(call::BERSERK_TRANSITION, 1);
            if !m.bool(self.at(a::EMOTIONS_BERSERK)) {
                self.handle(call::PANIC_FROM_ATTACHED_PROJECTILES);
                self.handle(call::PANIC_FROM_ATTACHED_MELEE_ATTACKERS);
                self.handle3(call::PANIC_TRANSITION, 9, 0, 11);
            }
            self.handle(call::COMBAT_TRANSITION);
            self.handle2(call::ACTIVE_COVER_SEEKING, 0, 0);
            self.handle(call::VEHICLE_ENTRY);
            self.handle(call::VEHICLE_EXIT);
            self.handle(call::GRENADE_THROWING);
            self.handle(call::DANGER_AVOIDANCE);
        }

        match self.action() {
            action::FIGHT | action::CHARGE => self.fight(),
            action::GUARD => {
                self.cower();
                let can_stop = self.can_stop_guarding();
                self.combat_status(can_stop, false);
            }
            action::FLEE => self.flee(),
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::VEHICLE => {
                if self.vehicle_entry_over() {
                    self.combat_status(true, true);
                }
            }
            action::OBEY => self.obey(),
            action::CONVERSE => self.converse(),
            action::AVOID => self.avoid(),
            _ => {}
        }
    }

    /// actor_type_jackal.c's `jackal_decide_action`.
    fn jackal(&self) {
        self.begin(Some(1));
        if !self.handle(call::DENY_TRANSITION) {
            self.panics(true);
            self.handle3(call::PANIC_TRANSITION, 1, 0, 4);
            self.handle(call::COMBAT_TRANSITION);
            self.handle2(call::ACTIVE_COVER_SEEKING, 1, 1);
            self.handle(call::VEHICLE_ENTRY);
            self.handle(call::VEHICLE_EXIT);
            self.handle(call::DANGER_AVOIDANCE);
        }

        match self.action() {
            action::FIGHT | action::CHARGE => self.fight(),
            action::GUARD => {
                self.cower();
                let can_stop = self.can_stop_guarding();
                self.combat_status(can_stop, false);
            }
            action::FLEE => self.flee(),
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::OBEY => self.obey(),
            action::CONVERSE => self.converse(),
            action::AVOID => self.avoid(),
            _ => {}
        }
    }

    /// actor_type_hunter.c's `hunter_decide_action`.
    fn hunter(&self) {
        self.begin(None);
        if !self.handle(call::DENY_TRANSITION) {
            self.handle(call::BERSERKING_FROM_DAMAGE);
            self.handle(call::BERSERKING_FROM_ATTACHED_PROJECTILES);
            self.handle1(call::BERSERK_TRANSITION, 3);
            self.handle(call::COMBAT_TRANSITION);
            self.handle(call::DANGER_AVOIDANCE);
        }

        match self.action() {
            action::FIGHT | action::FLEE | action::GUARD | action::CHARGE => {
                if !self.combat_status(true, false) {
                    self.handle(call::COMBAT_FAILURE);
                }
            }
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::OBEY => self.obey(),
            action::CONVERSE => self.converse(),
            action::AVOID => self.avoid(),
            _ => {}
        }
    }

    /// actor_type_engineer.c's `engineer_decide_action`.
    fn engineer(&self) {
        self.begin(Some(1));
        if !self.handle(call::DENY_TRANSITION) {
            self.panics(false);
            self.handle3(call::PANIC_TRANSITION, 1, 0, 9);
            self.handle(call::COMBAT_TRANSITION);
            self.handle(call::GRENADE_THROWING);
            self.handle(call::DANGER_AVOIDANCE);
        }
        self.infantry_actions();
    }

    /// The actions of the engineer, marine and crew (and much of the
    /// others').
    fn infantry_actions(&self) {
        match self.action() {
            action::FIGHT | action::CHARGE => self.fight(),
            action::GUARD => {
                let can_stop = self.can_stop_guarding();
                self.combat_status(can_stop, false);
            }
            action::FLEE => self.flee(),
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::VEHICLE => {
                if self.vehicle_entry_over() {
                    self.combat_status(true, true);
                }
            }
            action::OBEY => self.obey(),
            action::CONVERSE => self.converse(),
            action::AVOID => self.avoid(),
            _ => {}
        }
    }

    /// actor_type_marine.c's `marine_decide_action`.
    fn marine(&self) {
        let m = self.host.memory();
        self.begin(Some(1));
        if !self.handle(call::DENY_TRANSITION) {
            self.panics(false);
            self.handle3(call::PANIC_TRANSITION, 1, 0, 14);
            self.handle(call::BERSERKING_FROM_DAMAGE);
            let marines = m.i8(self.at(a::SITUATION_AREA_FRIENDS_BY_TYPE) + actor_type::MARINE as u32);
            self.handle1(call::BERSERK_TRANSITION, if marines > 2 { 5 } else { 3 });
            self.handle(call::COMBAT_TRANSITION);
            self.handle(call::VEHICLE_ENTRY);
            self.handle(call::VEHICLE_EXIT);
            self.handle(call::GRENADE_THROWING);
            self.handle(call::DANGER_AVOIDANCE);
        }
        self.infantry_actions();
    }

    /// actor_type_crew.c's `crew_decide_action`.
    fn crew(&self) {
        self.begin(Some(1));
        if !self.handle(call::DENY_TRANSITION) {
            self.panics(true);
            self.handle3(call::PANIC_TRANSITION, 1, 0, 9);
            self.handle(call::COMBAT_TRANSITION);
            self.handle(call::DANGER_AVOIDANCE);
        }
        self.infantry_actions();
    }

    /// actor_type_flood.c's `flood_decide_action`.
    fn flood(&self) {
        self.begin(Some(4));
        if !self.handle(call::DENY_TRANSITION) {
            self.handle(call::BERSERKING_FROM_DAMAGE);
            self.handle1(call::BERSERK_TRANSITION, 3);
            self.handle(call::COMBAT_TRANSITION);
            self.handle(call::DANGER_AVOIDANCE);
        }

        match self.action() {
            action::FIGHT | action::CHARGE => self.fight(),
            action::FLEE => self.flee(),
            action::GUARD => {
                let can_stop = self.can_stop_guarding();
                self.combat_status(can_stop, false);
            }
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::OBEY => self.obey(),
            action::AVOID => self.avoid(),
            _ => {}
        }
    }

    /// actor_type_infection.c's `infection_decide_action`.
    fn infection(&self) {
        self.begin(None);
        if !self.handle(call::DENY_TRANSITION) {
            self.handle(call::COMBAT_TRANSITION);
        }

        match self.action() {
            action::FIGHT | action::CHARGE => {
                if !self.combat_status(true, false) {
                    self.handle(call::COMBAT_FAILURE);
                }
            }
            action::FLEE => self.flee(),
            action::GUARD => {
                let can_stop = self.can_stop_guarding();
                self.combat_status(can_stop, false);
            }
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::OBEY => self.obey(),
            _ => {}
        }
    }

    /// actor_type_carrier.c's `carrier_decide_action`.
    fn carrier(&self) {
        self.begin(Some(4));
        if !self.handle(call::DENY_TRANSITION) {
            self.handle(call::BERSERKING_FROM_ATTACKING_MODE);
            self.handle(call::BERSERKING_FROM_DAMAGE);
            self.handle(call::BERSERKING_FROM_PROXIMITY);
            self.handle1(call::BERSERK_TRANSITION, 1);
            self.handle(call::COMBAT_TRANSITION);
        }

        match self.action() {
            action::FIGHT | action::CHARGE => self.fight(),
            action::GUARD => {
                let can_stop = self.can_stop_guarding();
                self.combat_status(can_stop, false);
            }
            action::FLEE => self.flee(),
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::OBEY => self.obey(),
            _ => {}
        }
    }

    /// actor_type_sentinel.c's `sentinel_decide_action`.
    fn sentinel(&self) {
        self.begin(Some(1));
        if !self.handle(call::DENY_TRANSITION) {
            self.handle(call::COMBAT_TRANSITION);
            self.handle(call::DANGER_AVOIDANCE);
        }

        match self.action() {
            action::FIGHT | action::CHARGE => self.fight(),
            action::GUARD => {
                let can_stop = self.can_stop_guarding();
                self.combat_status(can_stop, false);
            }
            action::FLEE => self.flee(),
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::OBEY => self.obey(),
            action::AVOID => self.avoid(),
            action::CONVERSE => self.converse(),
            _ => {}
        }
    }

    /// actor_type_mounted_weapon.c's `mounted_weapon_decide_action`.
    fn mounted_weapon(&self) {
        self.begin(None);
        if !self.handle(call::DENY_TRANSITION) {
            self.handle(call::COMBAT_TRANSITION);
        }

        match self.action() {
            action::FIGHT | action::FLEE | action::GUARD | action::CHARGE => {
                if !self.combat_status(true, false) {
                    self.handle(call::COMBAT_FAILURE);
                }
            }
            action::UNCOVER | action::SEARCH | action::WAIT => self.pursue(),
            action::OBEY => self.obey(),
            _ => {}
        }
    }
}

/// The pursuit settings of an actor of `actor_type` (actor_types.c's table:
/// its desires to search at the target, to pursue and to search the pursuit,
/// and whether it controls a group's pursuit).
pub fn pursuit_settings(actor_type: i16) -> (i16, i16, i16, u8) {
    match actor_type {
        actor_type::ELITE | actor_type::ASSASSIN | actor_type::HUNTER => (0, 1, 1, 1),
        actor_type::JACKAL => (2, 0, 2, 0),
        actor_type::COMBAT_FORM => (0, 0, 0, 1),
        actor_type::INFECTION_FORM | actor_type::MOUNTED_WEAPON => (2, 2, 2, 0),
        actor_type::CARRIER_FORM => (0, 2, 2, 1),
        // grunts, engineers, marines (and players), crew, sentinels (and
        // monitors), and none
        _ => (0, 0, 0, 0),
    }
}

/// actor_types.c's `actor_type_decide_action`: the actor's type's decision
/// (by actor_types.c's table: assassins decide as elites, players as
/// marines, monitors as sentinels, and none, or a type it lacks, as grunts).
pub fn actor_type_decide_action(context: &AiStepContext, host: &impl AiHost) {
    let decide = Decide { context, host };
    match host.memory().i16(context.actor + a::META_TYPE as u32) {
        actor_type::ELITE | actor_type::ASSASSIN => decide.elite(),
        actor_type::JACKAL => decide.jackal(),
        actor_type::HUNTER => decide.hunter(),
        actor_type::ENGINEER => decide.engineer(),
        actor_type::PLAYER | actor_type::MARINE => decide.marine(),
        actor_type::CREW => decide.crew(),
        actor_type::COMBAT_FORM => decide.flood(),
        actor_type::INFECTION_FORM => decide.infection(),
        actor_type::CARRIER_FORM => decide.carrier(),
        actor_type::MONITOR | actor_type::SENTINEL => decide.sentinel(),
        actor_type::MOUNTED_WEAPON => decide.mounted_weapon(),
        _ => decide.grunt(),
    }
}
