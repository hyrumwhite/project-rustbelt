//! A communication event (ai_communication.c's `ai_communication_event`):
//! when something happens to a unit (a death, damage, an enemy sighted, and
//! so on), who if anyone says what about it. Each of the event's dialogue
//! usages is tested against the hostility, the enemy's status, the races,
//! the damage and the teams' speech timers; its speaker (the subject, the
//! cause, a friend or an enemy) is found and weighed by the player's view
//! of it, its speech and animation, how recently the line was heard and the
//! reply's rating; one is chosen at random by weight (or among those that
//! force themselves) and spoken, or replied to for a player.
//!
//! The actors, encounters, the AI globals and the dialogue events' timers
//! are the game's memory, worked on in place ([`WorldContext`]; the dialogue
//! table is read from the game's own); the teams' allegiance and its
//! incidents, an actor type's race, finding an actor to talk or to reply
//! to a player, the player rating, the communication team, the speech and
//! animation tests, an actor's action class, the random seed, and speaking
//! or replying are the game's, through [`AiHost::call`] (ai_communication.c's
//! `communication_call`, shared with the C). The communication's debug
//! lines (`ai_debug.print_communication`) are the game's alone.

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext, datum_get, object_get};
use crate::layout::{actor_datum as a, ai_globals as g, ai_values as v, encounter_datum as e, unit_datum as u};

/// The communication event's calls (halo_sim.h's `_communication_call_*`).
/// Several pass integers in `input` as their bits.
pub mod call {
    /// `game_team_is_ally(a, b)`, the result.
    pub const TEAM_IS_ALLY: i32 = 1;
    /// `game_team_is_enemy(a, b)`, the result.
    pub const TEAM_IS_ENEMY: i32 = 2;
    /// `actor_type_get_race(a)`, the result.
    pub const ACTOR_TYPE_RACE: i32 = 3;
    /// `ai_communication_find_global_actor_to_talk(a, input: mode, subject,
    /// cause, maximum distance (a real), type, priority, speech priority,
    /// vocalization, animation, b: flags)`, the result.
    pub const FIND_GLOBAL_ACTOR: i32 = 4;
    /// `ai_communication_find_specific_actor_to_talk(a: the encounter's
    /// absolute index, input as above but the mode, b: flags)`, the result.
    pub const FIND_SPECIFIC_ACTOR: i32 = 5;
    /// The allegiance incident of a death between allied teams (the cause's
    /// team `a`, the subject's `b`; `input[0]` betrayal, `input[1]`
    /// observed): its debug line, and the incident when observed.
    pub const INCIDENT: i32 = 6;
    /// `sound_scripted_dialog_is_playing()`, the result.
    pub const SCRIPTED_DIALOG_PLAYING: i32 = 7;
    /// `ai_communication_find_actor_to_reply_to_player(a, b, input[0]:
    /// vocalization, input[1]: damage)`: the result, the reply rating in
    /// `output[0]`.
    pub const FIND_REPLY_ACTOR: i32 = 8;
    /// `ai_communication_get_player_rating(a, TRUE, NULL, NULL)`, in
    /// `output[0]`.
    pub const PLAYER_RATING: i32 = 9;
    /// `actor_communication_team(a)`, the result.
    pub const COMMUNICATION_TEAM: i32 = 10;
    /// `ai_communication_consider_speech(a, input: priority, speech
    /// priority, delay, lookup, vocalization)`: the play type; the weight,
    /// the vocalization and the sound in `output[0..3]`.
    pub const CONSIDER_SPEECH: i32 = 11;
    /// `unit_test_animation_impulse(a, b)`, the result.
    pub const TEST_ANIMATION_IMPULSE: i32 = 12;
    /// `actor_action_class(a)`, the result.
    pub const ACTION_CLASS: i32 = 13;
    /// `real_seed_random(global seed)`, in `output[0]`.
    pub const RANDOM_REAL: i32 = 14;
    /// The chosen reply (the selected possibility in the context's
    /// `communication_selected`) started, notified and finished for the
    /// event's type `a`, damage `b` and information type `input[0]`.
    pub const REPLY: i32 = 15;
    /// The chosen speech spoken (the selected possibility as above): the
    /// unit's speech, its animation, the speaker's look and the speech
    /// timers.
    pub const SPEAK: i32 = 16;
}

/// ai_communication.c's enums and tables.
const PRIORITY_NONE: usize = 0;
const PRIORITY_YELL: usize = 6;
const PRIORITY_EXCLAIM: usize = 7;
const HOSTILITY_NONE: i16 = 0;
const HOSTILITY_ENEMY: i16 = 3;
const HOSTILITY_TRAITOR: i16 = 4;
const COMMUNICATION_DEATH: i32 = 0;
const FIND_ACTOR_MODE_SAME_TEAM: i32 = 0;
const FIND_ACTOR_MODE_FRIEND: i32 = 1;
const FIND_ACTOR_MODE_ENEMY: i32 = 2;
const FIND_ACTOR_ALLOW_LOOKUP_BIT: i32 = 0;
const FIND_ACTOR_NEAR_TO_PLAYERS_BIT: i32 = 1;
const FIND_ACTOR_SAME_VEHICLE_BIT: i32 = 2;
const FIND_ACTOR_ALLOW_SUBJECT_BIT: i32 = 3;
const FIND_ACTOR_ALLOW_CAUSE_BIT: i32 = 4;
const ACTOR_MODE_BRAINDEAD: i16 = 0;
const ACTOR_MODE_ASLEEP: i16 = 1;
const COMBAT_STATUS_DEFINITE: i16 = 3;
const COMBAT_STATUS_DANGEROUS: i16 = 6;
const INFORMATION_NONE: i16 = 0;
const INFORMATION_LOOK_UNIT: i16 = 1;
const INFORMATION_LOOK_OBJECT: i16 = 2;
const USAGE_LOOKUP_BIT: i16 = 0;
const USAGE_FORCE_BIT: i16 = 1;
const USAGE_IMMEDIATE_NOTIFY_BIT: i16 = 2;
const USAGE_PLAYER_BIT: i16 = 3;
const USAGE_SAME_VEHICLE_BIT: i16 = 4;
const USAGE_ALLOW_SUBJECT_BIT: i16 = 5;
const USAGE_OVERRIDE_SCRIPTED_BIT: i16 = 6;
const LOOK_DIRECTION_SUBJECT: i16 = 1;
const LOOK_DIRECTION_PROTAGONIST: i16 = 2;
const LOOK_DIRECTION_TARGET: i16 = 3;
const LOOK_DIRECTION_DANGER: i16 = 4;
const PROTAGONIST_SUBJECT: i16 = 0;
const PROTAGONIST_CAUSE: i16 = 1;
const PROTAGONIST_FRIEND: i16 = 2;
const PROTAGONIST_ENEMY: i16 = 4;
const PLAY_SPEECH_NONE: i16 = 0;
const ACTION_CLASS_TRANSITORY: i32 = 2;
const RACE_PLAYER: i16 = 0x1;
const TEAMS: usize = 2;
const PRIORITIES: usize = 8;
const MAXIMUM_POSSIBILITIES: usize = 16;
const TICKS_PER_SECOND: f32 = 30.0;

const SPEECH_PRIORITIES: [i16; PRIORITIES] = [0, 3, 3, 3, 4, 5, 5, 8];
const NOTIFICATION_DELAYS: [f32; PRIORITIES] = [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.3];
const PROTAGONIST_DEFAULT_LOOK_PRIORITIES: [i16; PRIORITIES] = [0, 4, 4, 5, 6, 6, 6, 4];
const RECIPIENT_DEFAULT_LOOK_PRIORITIES: [i16; PRIORITIES] = [0, 3, 3, 4, 5, 5, 6, 4];
const PLAYER_SPEAKING_PRIORITIES: [usize; PRIORITIES] = [0, 2, 3, 4, 4, 6, 6, 7];
const TIMER_TOLERANCES: [[[f32; 5]; 2]; PRIORITIES] = [
    [[0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0]],
    [[4.0, 4.5, 2.5, 2.0, 0.0], [5.0, 5.0, 3.0, 2.5, 0.0]],
    [[3.0, 3.5, 2.0, 1.5, 0.0], [4.5, 4.5, 2.5, 2.0, 0.0]],
    [[0.3, 1.0, 1.0, 1.0, 0.8], [1.0, 2.0, 1.5, 1.5, 0.3]],
    [[0.0, 0.5, 0.5, 0.5, 1.3], [0.2, 1.0, 1.0, 1.0, 0.8]],
    [[0.0, 0.0, 1.0, 1.0, 1.5], [0.0, 0.0, 1.5, 1.5, 1.0]],
    [[0.0, 0.0, 0.5, 0.0, 1.5], [0.0, 0.0, 0.5, 0.0, 1.5]],
    [[0.0, 0.0, 0.5, 0.0, 0.0], [0.0, 0.0, 0.5, 0.0, 0.0]],
];
const PLAY_DELAYS: [f32; 5] = [0.0, 0.5, 0.8, 0.5, 0.8];
const PLAYER_ADDITIONAL_DELAY: i16 = 30;
const TIMEOUT_LOW_PRIORITY_MODIFIER: i32 = 30;
const PLAYER_RATING_LOW_PRIORITY: f32 = 2.0;

/// [`WorldContext::communication_flags`]: a local game's debug suppressing
/// this communication type, and disabling the dialogue timeouts.
const FLAG_SUPPRESSED: u32 = 1 << 0;
const FLAG_TIMEOUT_DISABLED: u32 = 1 << 1;

/// ai_communication.c's `struct dialogue_usage` (asserted in the C).
mod usage {
    pub const SIZEOF: u32 = 0x28;
    pub const COMMUNICATION_TYPE: u32 = 0x0;
    pub const COMMUNICATION_PRIORITY: u32 = 0x2;
    pub const VOCALIZATION_TYPE: u32 = 0x4;
    pub const ANIMATION_TYPE: u32 = 0x6;
    pub const PROTAGONIST_TYPE: u32 = 0x8;
    pub const PROTAGONIST_LOOK_PRIORITY: u32 = 0xA;
    pub const RECIPIENT_LOOK_DIRECTION: u32 = 0xC;
    pub const RECIPIENT_LOOK_PRIORITY: u32 = 0xE;
    pub const WEIGHT: u32 = 0x10;
    pub const FLAGS: u32 = 0x18;
    pub const REQUIRED_GROUP: u32 = 0x1A;
    pub const REQUIRED_HOSTILITY: u32 = 0x1C;
    pub const REQUIRED_ENEMY_STATUS: u32 = 0x1E;
    pub const REQUIRED_SUBJECT_RACE: u32 = 0x20;
    pub const REQUIRED_CAUSE_RACE: u32 = 0x22;
    pub const REQUIRED_DAMAGE: u32 = 0x24;
}

/// ai_communication.c's `struct ai_communication_possibility` (asserted
/// in the C).
#[derive(Clone, Copy, Default)]
struct Possibility {
    weight: f32,
    interrupts: bool,
    is_reply: bool,
    vocalization_type: i16,
    priority: i16,
    animation_type: i16,
    play_type: i16,
    delay_time: i16,
    ai_delay_time: i16,
    protagonist_unit_index: i32,
    protagonist_actor_index: i32,
    recipient_unit_index: i32,
    preselected_reply_actor_index: i32,
    protagonist_look_priority: i16,
    recipient_look_priority: i16,
    look_type: i16,
    look_unit_index: i32,
    sound_definition_index: i32,
    dialogue_type_index: i16,
}

impl Possibility {
    /// Written out as the C's port struct holds it (its padding zero).
    fn write(&self, m: &dyn AiMemory, address: u32) {
        m.set_f32(address, self.weight);
        m.set_bool(address + 0x4, self.interrupts);
        m.set_bool(address + 0x5, self.is_reply);
        m.set_i16(address + 0x6, self.vocalization_type);
        m.set_i16(address + 0x8, self.priority);
        m.set_i16(address + 0xA, self.animation_type);
        m.set_i16(address + 0xC, self.play_type);
        m.set_i16(address + 0xE, self.delay_time);
        m.set_i16(address + 0x10, self.ai_delay_time);
        m.w16(address + 0x12, 0);
        m.w32(address + 0x14, self.protagonist_unit_index as u32);
        m.w32(address + 0x18, self.protagonist_actor_index as u32);
        m.w32(address + 0x1C, self.recipient_unit_index as u32);
        m.w32(address + 0x20, self.preselected_reply_actor_index as u32);
        m.set_i16(address + 0x24, self.protagonist_look_priority);
        m.set_i16(address + 0x26, self.recipient_look_priority);
        m.set_i16(address + 0x28, self.look_type);
        m.w16(address + 0x2A, 0);
        m.w32(address + 0x2C, self.look_unit_index as u32);
        m.w32(address + 0x30, self.sound_definition_index as u32);
        m.set_i16(address + 0x34, self.dialogue_type_index);
        m.w16(address + 0x36, 0);
    }
}

/// ai.c's `ai_get_race_from_team_index`.
fn race_from_team(team_index: i16) -> i16 {
    match team_index {
        1 => 0x1,
        2 => 0x2,
        3 => 0x4,
        4 => 0x38,
        5 => 0x40,
        _ => 0,
    }
}

fn bits(value: i32) -> f32 {
    f32::from_bits(value as u32)
}

fn unbits(value: f32) -> i32 {
    value.to_bits() as i32
}

/// A unit's side of the event.
#[derive(Clone, Copy)]
struct Party {
    actor_index: i32,
    actor: u32,
    team: i16,
    race: i16,
    groups: [bool; 2],
}

/// `ai_communication_event` on the context's arguments.
pub fn ai_communication_event(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let step = context.step_context();
    let [communication_type, subject_unit_index, cause_unit_index, hostility_argument, damage_argument, information_type] =
        context.communication;
    let call = |kind: i32, a: i32, b: i32, input: &[f32]| {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        host.call(&mut call);
        call
    };
    let event_time = context.game_time;
    let mut hostility = if hostility_argument as i16 == NONE as i16 { HOSTILITY_NONE } else { hostility_argument as i16 };
    let damage_type = if damage_argument as i16 == NONE as i16 { 0 } else { damage_argument as i16 };
    let mut find_friend_actor = true;
    let mut find_other_actor = true;
    let mut friend_actor_index = NONE;
    let mut other_actor_index = NONE;

    let party = |unit_index: i32| -> Option<Party> {
        if unit_index == NONE {
            return None;
        }
        let unit = object_get(m, &step, unit_index).unwrap_or(0);
        let actor_index = m.i32(unit + u::UNIT_ACTOR_INDEX as u32);
        let team = m.i16(unit + u::OBJECT_OWNER_TEAM_INDEX as u32);
        let mut party = Party {
            actor_index,
            actor: 0,
            team,
            race: race_from_team(team),
            groups: [false; 2],
        };
        if actor_index != NONE {
            let actor = datum_get(m, context.actor_data, actor_index).unwrap_or(0);
            party.actor = actor;
            party.race = call(call::ACTOR_TYPE_RACE, m.i16(actor + a::META_TYPE as u32) as i32, 0, &[]).result as i16;
            if m.i8(actor + a::SITUATION_CLOSE_FRIENDS as u32) > 0 {
                party.groups = [true, true];
            } else if m.i8(actor + a::SITUATION_AREA_FRIENDS as u32) > 0 {
                party.groups = [true, false];
            }
        } else if m.i32(unit + u::UNIT_PLAYER_INDEX as u32) != NONE {
            party.race = RACE_PLAYER;
        }
        Some(party)
    };
    let subject = party(subject_unit_index);
    let subject_encounter_index = subject
        .filter(|s| s.actor_index != NONE)
        .map_or(NONE, |s| m.i32(s.actor + a::META_ENCOUNTER_INDEX as u32));
    let subject_encounter = (subject_encounter_index != NONE)
        .then(|| datum_get(m, context.encounter_data, subject_encounter_index).unwrap_or(0));
    let cause = party(cause_unit_index);
    let subject_team = subject.map_or(NONE as i16, |s| s.team);
    let subject_race = subject.map_or(0, |s| s.race);
    let cause_race = cause.map_or(0, |c| c.race);
    let subject_actor_index = subject.map_or(NONE, |s| s.actor_index);
    let cause_actor_index = cause.map_or(NONE, |c| c.actor_index);

    if let (Some(subject), Some(cause)) = (subject, cause)
        && subject.team != cause.team
        && call(call::TEAM_IS_ALLY, subject.team as i32, cause.team as i32, &[]).result != 0
    {
        let mut betrayal = false;
        let mut observed = false;
        if communication_type == COMMUNICATION_DEATH {
            if hostility == HOSTILITY_ENEMY {
                betrayal = true;
                observed = true;
            } else {
                if let Some(encounter) = subject_encounter {
                    let timer = m.i32(encounter + e::ENEMY_VISIBLE_TIMER as u32);
                    betrayal = m.bool(encounter + e::ENEMY_TRAITOR as u32) || timer == NONE || timer >= 270;
                }
                friend_actor_index = call(
                    call::FIND_GLOBAL_ACTOR,
                    subject.team as i32,
                    0,
                    &[
                        bits(FIND_ACTOR_MODE_SAME_TEAM),
                        bits(subject_unit_index),
                        bits(cause_unit_index),
                        18.0,
                        bits(COMMUNICATION_DEATH),
                        bits(PRIORITY_YELL as i32),
                        bits(NONE),
                        bits(NONE),
                        bits(NONE),
                    ],
                )
                .result;
                if friend_actor_index != NONE {
                    find_friend_actor = false;
                    observed = true;
                }
                if matches!(damage_type, 3 | 4 | 9) && !betrayal {
                    observed = false;
                }
                if damage_type == 3 {
                    betrayal = false;
                }
            }
            if betrayal {
                hostility = HOSTILITY_TRAITOR;
            }
            call(call::INCIDENT, cause.team as i32, subject.team as i32, &[bits(betrayal as i32), bits(observed as i32)]);
        }
        if call(call::TEAM_IS_ENEMY, subject.team as i32, cause.team as i32, &[]).result != 0 {
            hostility = HOSTILITY_TRAITOR;
        }
    }

    let mut enemy_status_enabled = [true; 6];
    if let Some(subject) = subject.filter(|s| s.actor_index != NONE) {
        let actor = subject.actor;
        let at = |offset: usize| actor + offset as u32;
        let combat_status = m.i16(at(a::STATE_COMBAT_STATUS));
        let since_visible = m.i32(at(a::TARGET_SINCE_ANY_TARGET_VISIBLE_TIMER));
        let really_alive = m.bool(at(a::TARGET_TARGET_REALLY_ALIVE));
        enemy_status_enabled[0] = !m.bool(at(a::TARGET_ANY_TARGET_EVER));
        match subject_encounter {
            None => {
                enemy_status_enabled[1] = !really_alive && since_visible != NONE;
                enemy_status_enabled[2] =
                    m.i32(at(a::TARGET_TARGET_PROP_INDEX)) == NONE || since_visible == NONE || since_visible >= 180;
                enemy_status_enabled[3] = combat_status < COMBAT_STATUS_DEFINITE
                    && (since_visible == NONE || since_visible >= 75)
                    && (really_alive || combat_status > 0);
                enemy_status_enabled[4] = combat_status < COMBAT_STATUS_DANGEROUS;
                enemy_status_enabled[5] =
                    m.i16(at(a::TARGET_TARGET_TYPE)) >= v::_ACTOR_TARGET_VISIBLE_ENEMY as i16 && really_alive;
            }
            Some(encounter) => {
                let timer = m.i32(encounter + e::ENEMY_VISIBLE_TIMER as u32);
                let alive = m.bool(encounter + e::ENEMY_ALIVE as u32);
                enemy_status_enabled[1] = timer != NONE && !alive;
                enemy_status_enabled[2] = (timer == NONE || timer >= 180) && alive;
                enemy_status_enabled[3] = combat_status < COMBAT_STATUS_DEFINITE
                    && (timer == NONE || timer >= 75)
                    && (alive || combat_status > 0);
                enemy_status_enabled[4] = combat_status < COMBAT_STATUS_DANGEROUS && (timer == NONE || timer >= 75);
                enemy_status_enabled[5] = m.bool(encounter + e::ENEMY_VISIBLE as u32) && alive;
            }
        }
    }

    let mut hostility_enabled = [false; 5];
    hostility_enabled[hostility as usize] = true;
    if hostility == HOSTILITY_TRAITOR {
        hostility_enabled[HOSTILITY_ENEMY as usize] = true;
    }

    let mut speech_disabled = [[[false; 2]; PRIORITIES]; TEAMS];
    let mut speech_delay = [[[0i16; 2]; PRIORITIES]; TEAMS];
    for team in 0..TEAMS {
        let since = |offset: usize| {
            let last = m.i32(context.ai_globals + offset as u32 + 4 * team as u32);
            event_time.wrapping_sub(last).max(0) as i16
        };
        let time_since_shout = since(g::LAST_SHOUT_TIME);
        let time_since_talk = since(g::LAST_TALK_TIME);
        let time_since_chatter = since(g::LAST_CHATTER_TIME);
        speech_disabled[team][PRIORITY_NONE] = [true, true];
        for priority in 1..PRIORITY_YELL {
            for group in 0..2 {
                let tolerances = TIMER_TOLERANCES[priority][group];
                let mut disabled = false;
                let mut delay = 0i16;
                for (tolerance, since) in [
                    (tolerances[0], time_since_chatter),
                    (tolerances[1], time_since_talk),
                    (tolerances[3], time_since_shout),
                ] {
                    if tolerance > 0.0 {
                        let remaining = (tolerance * TICKS_PER_SECOND - since as f32) as i32 as i16;
                        if remaining > 0 {
                            disabled = true;
                            delay = delay.max(remaining);
                        }
                    }
                }
                // (the original reads the slot's stored delay, still zero,
                // before writing it)
                if disabled
                    && tolerances[4] > 0.0
                    && (speech_delay[team][priority][group] as f32) < tolerances[4] * TICKS_PER_SECOND
                {
                    disabled = false;
                }
                speech_disabled[team][priority][group] = disabled;
                speech_delay[team][priority][group] = delay;
            }
        }
    }

    if !m.bool(context.ai_globals + g::DIALOGUE_TRIGGERS_ENABLED as u32) {
        return;
    }

    let mut dialogue_index = m.i16(context.communication_table_indices + 2 * communication_type as u32);
    if context.communication_flags & FLAG_SUPPRESSED != 0 {
        dialogue_index = NONE as i16;
    }

    let mut possibilities: Vec<Possibility> = Vec::new();
    let mut total_weight = 0.0f32;
    let mut any_forced = false;

    if dialogue_index != NONE as i16 {
        loop {
            let usage_address = context.dialogue_table + usage::SIZEOF * dialogue_index as u32;
            let field = |offset: u32| m.i16(usage_address + offset);
            if field(usage::COMMUNICATION_TYPE) as i32 != communication_type {
                break;
            }
            let this_index = dialogue_index;
            dialogue_index += 1;
            let usage_flags = field(usage::FLAGS);
            let flag = |bit: i16| usage_flags & (1 << bit) != 0;
            let mut communication_priority = field(usage::COMMUNICATION_PRIORITY) as usize;
            let vocalization_type_usage = field(usage::VOCALIZATION_TYPE);
            let animation_type_usage = field(usage::ANIMATION_TYPE);

            let required_hostility = field(usage::REQUIRED_HOSTILITY);
            let required_enemy_status = field(usage::REQUIRED_ENEMY_STATUS);
            let required_subject_race = field(usage::REQUIRED_SUBJECT_RACE);
            let required_cause_race = field(usage::REQUIRED_CAUSE_RACE);
            let required_damage = field(usage::REQUIRED_DAMAGE);
            if required_hostility != NONE as i16 && !hostility_enabled[required_hostility as usize] {
                continue;
            }
            if call(call::SCRIPTED_DIALOG_PLAYING, 0, 0, &[]).result != 0
                && communication_priority < PRIORITY_YELL
                && !flag(USAGE_OVERRIDE_SCRIPTED_BIT)
            {
                continue;
            }
            if required_enemy_status != NONE as i16 && !enemy_status_enabled[required_enemy_status as usize] {
                continue;
            }
            if required_subject_race != NONE as i16
                && (subject_unit_index == NONE || required_subject_race & subject_race == 0)
            {
                continue;
            }
            if required_cause_race != NONE as i16 && (cause_unit_index == NONE || required_cause_race & cause_race == 0)
            {
                continue;
            }
            if required_damage != NONE as i16 && required_damage != damage_type {
                continue;
            }

            let speech_priority = SPEECH_PRIORITIES[communication_priority];
            let mut protagonist_actor_index = NONE;
            let mut protagonist_unit_index = NONE;
            let mut protagonist_actor = 0u32;
            let mut recipient_unit_index = NONE;
            let mut protagonist_groups: Option<[bool; 2]> = None;
            let mut is_reply = false;
            let mut reply_actor_index = NONE;
            let mut candidate_delay = 0i16;
            let mut repeat_rating = 1.0f32;
            let mut reply_rating = 1.0f32;
            let protagonist_type = field(usage::PROTAGONIST_TYPE);
            let find_flags = |cause_allowed: bool| {
                let mut flags = 0i32;
                if flag(USAGE_LOOKUP_BIT) {
                    flags |= 1 << FIND_ACTOR_ALLOW_LOOKUP_BIT;
                }
                flags |= 1 << FIND_ACTOR_NEAR_TO_PLAYERS_BIT;
                if flag(USAGE_SAME_VEHICLE_BIT) {
                    flags |= 1 << FIND_ACTOR_SAME_VEHICLE_BIT;
                }
                if flag(USAGE_ALLOW_SUBJECT_BIT) {
                    flags |= 1 << FIND_ACTOR_ALLOW_SUBJECT_BIT;
                }
                if cause_allowed {
                    flags |= 1 << FIND_ACTOR_ALLOW_CAUSE_BIT;
                }
                flags
            };
            let actor_of = |actor_index: i32| datum_get(m, context.actor_data, actor_index).unwrap_or(0);

            match protagonist_type {
                PROTAGONIST_SUBJECT => {
                    if let Some(subject) = subject {
                        protagonist_groups = Some(subject.groups);
                    } else {
                        protagonist_groups = Some([false; 2]);
                    }
                    protagonist_unit_index = subject_unit_index;
                    protagonist_actor_index = subject_actor_index;
                    protagonist_actor = subject.map_or(0, |s| s.actor);
                    recipient_unit_index = cause_unit_index;
                }
                PROTAGONIST_CAUSE => {
                    protagonist_groups = Some(cause.map_or([false; 2], |c| c.groups));
                    protagonist_unit_index = cause_unit_index;
                    protagonist_actor_index = cause_actor_index;
                    protagonist_actor = cause.map_or(0, |c| c.actor);
                    recipient_unit_index = subject_unit_index;
                }
                PROTAGONIST_FRIEND => {
                    recipient_unit_index = cause_unit_index;
                    if find_friend_actor {
                        let flags = find_flags(true);
                        let rest = [
                            bits(subject_unit_index),
                            bits(cause_unit_index),
                            10.0,
                            bits(communication_type),
                            bits(communication_priority as i32),
                            bits(speech_priority as i32),
                            bits(vocalization_type_usage as i32),
                            bits(animation_type_usage as i32),
                        ];
                        friend_actor_index = if subject_encounter_index != NONE {
                            let mut input = [0.0f32; 9];
                            input[1..].copy_from_slice(&rest);
                            call(call::FIND_SPECIFIC_ACTOR, (subject_encounter_index & 0xFFFF) as i16 as i32, flags, &input)
                                .result
                        } else {
                            let mut input = [bits(FIND_ACTOR_MODE_FRIEND); 9];
                            input[1..].copy_from_slice(&rest);
                            call(call::FIND_GLOBAL_ACTOR, subject_team as i32, flags, &input).result
                        };
                        find_friend_actor = false;
                    }
                    protagonist_actor_index = friend_actor_index;
                    if protagonist_actor_index != NONE {
                        protagonist_actor = actor_of(protagonist_actor_index);
                        protagonist_unit_index = m.i32(protagonist_actor + a::META_UNIT_INDEX as u32);
                    }
                }
                PROTAGONIST_ENEMY => {
                    recipient_unit_index = cause_unit_index;
                    if find_other_actor {
                        let flags = find_flags(false);
                        other_actor_index = call(
                            call::FIND_GLOBAL_ACTOR,
                            subject_team as i32,
                            flags,
                            &[
                                bits(FIND_ACTOR_MODE_ENEMY),
                                bits(subject_unit_index),
                                bits(cause_unit_index),
                                12.0,
                                bits(communication_type),
                                bits(communication_priority as i32),
                                bits(speech_priority as i32),
                                bits(vocalization_type_usage as i32),
                                bits(animation_type_usage as i32),
                            ],
                        )
                        .result;
                        find_other_actor = false;
                    }
                    protagonist_actor_index = other_actor_index;
                    if protagonist_actor_index != NONE {
                        protagonist_actor = actor_of(protagonist_actor_index);
                        protagonist_unit_index = m.i32(protagonist_actor + a::META_UNIT_INDEX as u32);
                    }
                }
                _ => {}
            }

            let mut protagonist_invalid = false;
            if protagonist_unit_index == NONE {
                protagonist_invalid = true;
            } else {
                let unit = object_get(m, &step, protagonist_unit_index).unwrap_or(0);
                if m.r16(unit + u::OBJECT_DAMAGE_FLAGS as u32) & (1 << v::_OBJECT_DEAD_BIT) != 0 {
                    protagonist_invalid = true;
                } else if m.i16(unit + u::OBJECT_TYPE as u32) == v::_OBJECT_TYPE_VEHICLE as i16 {
                    protagonist_invalid = true;
                } else if m.i32(unit + u::UNIT_PLAYER_INDEX as u32) != NONE && m.i32(unit + u::UNIT_ACTOR_INDEX as u32) == NONE {
                    if flag(USAGE_PLAYER_BIT) {
                        is_reply = true;
                    } else {
                        protagonist_invalid = true;
                    }
                }
            }
            if protagonist_actor != 0 {
                if m.i16(protagonist_actor + a::STATE_MODE as u32) == ACTOR_MODE_BRAINDEAD {
                    protagonist_invalid = true;
                } else if m.i16(protagonist_actor + a::STATE_ACTION as u32) == v::_ACTOR_ACTION_OBEY as i16
                    && !m.bool(protagonist_actor + a::STATE_OBEY_ALLOW_COMMUNICATION as u32)
                {
                    protagonist_invalid = true;
                }
            }
            if protagonist_invalid {
                continue;
            }

            if is_reply {
                let reply = call(
                    call::FIND_REPLY_ACTOR,
                    protagonist_unit_index,
                    recipient_unit_index,
                    &[bits(vocalization_type_usage as i32), bits(damage_type as i32)],
                );
                reply_actor_index = reply.result;
                reply_rating = reply.output[0];
                if protagonist_unit_index == subject_unit_index {
                    find_friend_actor = false;
                    friend_actor_index = reply_actor_index;
                }
                if reply_actor_index == NONE {
                    continue;
                }
            }

            let required_group = field(usage::REQUIRED_GROUP);
            if required_group != NONE as i16
                && let Some(groups) = protagonist_groups
                && !groups[required_group as usize]
            {
                continue;
            }

            let player_rating;
            if is_reply {
                communication_priority = PLAYER_SPEAKING_PRIORITIES[communication_priority];
                player_rating = 2.0f32;
            } else {
                player_rating = call(call::PLAYER_RATING, protagonist_unit_index, 0, &[]).output[0];
                if player_rating == 0.0 {
                    continue;
                }
                let near_player = player_rating < PLAYER_RATING_LOW_PRIORITY;
                let communication_team = if protagonist_actor_index == NONE {
                    NONE
                } else {
                    call(call::COMMUNICATION_TEAM, protagonist_actor_index, 0, &[]).result as i16 as i32
                };
                if communication_team != NONE {
                    let team = communication_team as usize;
                    if speech_disabled[team][communication_priority][near_player as usize] {
                        continue;
                    }
                    candidate_delay = speech_delay[team][communication_priority][near_player as usize];
                    if communication_priority < PRIORITY_EXCLAIM {
                        let event = context.dialogue_events + 8 * (this_index as u32 * TEAMS as u32 + team as u32);
                        let last_time_spoken = m.i32(event);
                        if last_time_spoken != NONE {
                            repeat_rating = event_time.wrapping_sub(last_time_spoken) as f32 * 0.0011111111;
                            if repeat_rating < 0.0 {
                                repeat_rating = 0.0;
                            } else if repeat_rating > 1.0 {
                                repeat_rating = 1.0;
                            }
                        }
                        let disable_until_time = m.i32(event + 4);
                        if context.communication_flags & FLAG_TIMEOUT_DISABLED == 0 && disable_until_time != NONE {
                            let mut remaining = disable_until_time.wrapping_sub(event_time);
                            if near_player {
                                remaining += TIMEOUT_LOW_PRIORITY_MODIFIER;
                            }
                            if remaining > 0 {
                                continue;
                            }
                        }
                    }
                }
            }

            let mut delay_time = (PLAY_DELAYS[protagonist_type as usize] * TICKS_PER_SECOND) as i32 as i16;
            if subject_race == RACE_PLAYER && !is_reply {
                delay_time = delay_time.wrapping_add(PLAYER_ADDITIONAL_DELAY);
            }
            let mut ai_delay_time = (if flag(USAGE_IMMEDIATE_NOTIFY_BIT) {
                0.0
            } else {
                NOTIFICATION_DELAYS[communication_priority] * TICKS_PER_SECOND
            }) as i32 as i16;
            delay_time = delay_time.wrapping_add(candidate_delay);
            ai_delay_time = ai_delay_time.wrapping_add(candidate_delay);

            let mut look_type = INFORMATION_NONE;
            let mut look_unit_index = NONE;
            match field(usage::RECIPIENT_LOOK_DIRECTION) {
                LOOK_DIRECTION_SUBJECT => {
                    if subject_unit_index != NONE {
                        look_type = INFORMATION_LOOK_UNIT;
                        look_unit_index = subject_unit_index;
                    }
                }
                LOOK_DIRECTION_PROTAGONIST => {
                    if protagonist_unit_index != NONE {
                        look_type = INFORMATION_LOOK_UNIT;
                        look_unit_index = protagonist_unit_index;
                    }
                }
                LOOK_DIRECTION_TARGET => {
                    if recipient_unit_index != NONE {
                        look_type = INFORMATION_LOOK_UNIT;
                        look_unit_index = recipient_unit_index;
                    }
                }
                LOOK_DIRECTION_DANGER => {
                    if subject_actor_index != NONE {
                        let actor = actor_of(subject_actor_index);
                        if m.i16(actor + a::DANGER_ZONE_DANGER_TYPE as u32) > v::_ACTOR_DANGER_ZONE_NONE as i16 {
                            look_type = INFORMATION_LOOK_OBJECT;
                            look_unit_index = m.i32(actor + a::DANGER_ZONE_OBJECT_INDEX as u32);
                        }
                    }
                }
                _ => {}
            }
            let mut recipient_look_priority = 0i16;
            if look_type != INFORMATION_NONE {
                recipient_look_priority = field(usage::RECIPIENT_LOOK_PRIORITY);
                if recipient_look_priority == NONE as i16 || recipient_look_priority == 1 {
                    recipient_look_priority = RECIPIENT_DEFAULT_LOOK_PRIORITIES[communication_priority];
                }
            }
            let mut protagonist_look_priority = field(usage::PROTAGONIST_LOOK_PRIORITY);
            if protagonist_look_priority == NONE as i16 || protagonist_look_priority == 1 {
                protagonist_look_priority = PROTAGONIST_DEFAULT_LOOK_PRIORITIES[communication_priority];
            }

            let mut vocalization_type = vocalization_type_usage;
            let mut sound_definition_index = NONE;
            let mut speech_weight = 1.0f32;
            let mut animation_weight = 1.0f32;
            let mut play_type = PLAY_SPEECH_NONE;
            if !is_reply {
                let speech = call(
                    call::CONSIDER_SPEECH,
                    protagonist_unit_index,
                    0,
                    &[
                        bits(communication_priority as i32),
                        bits(speech_priority as i32),
                        bits(delay_time as i32),
                        bits(flag(USAGE_LOOKUP_BIT) as i32),
                        bits(vocalization_type as i32),
                    ],
                );
                play_type = speech.result as i16;
                speech_weight = speech.output[0];
                vocalization_type = unbits(speech.output[1]) as i16;
                sound_definition_index = unbits(speech.output[2]);
                if play_type == PLAY_SPEECH_NONE {
                    continue;
                } else if animation_type_usage != NONE as i16 {
                    let mut animation_impulse = call(
                        call::TEST_ANIMATION_IMPULSE,
                        protagonist_unit_index,
                        animation_type_usage as i32,
                        &[],
                    )
                    .result
                        != 0;
                    if animation_impulse && protagonist_actor_index != NONE {
                        if call(call::ACTION_CLASS, protagonist_actor_index, 0, &[]).result == ACTION_CLASS_TRANSITORY {
                            animation_impulse = false;
                        } else if m.i16(protagonist_actor + a::STATE_MODE as u32) == ACTOR_MODE_ASLEEP {
                            animation_impulse = false;
                        }
                    }
                    if animation_impulse {
                        animation_weight = 2.0;
                    }
                }
            }

            let weight = animation_weight
                * speech_weight
                * player_rating
                * m.f32(usage_address + usage::WEIGHT)
                * reply_rating
                * repeat_rating;
            if weight > 0.0 {
                if possibilities.len() >= MAXIMUM_POSSIBILITIES {
                    break;
                }
                let interrupts = flag(USAGE_FORCE_BIT);
                if interrupts {
                    any_forced = true;
                }
                possibilities.push(Possibility {
                    weight,
                    interrupts,
                    is_reply,
                    vocalization_type,
                    priority: speech_priority,
                    animation_type: animation_type_usage,
                    play_type,
                    delay_time,
                    ai_delay_time,
                    protagonist_unit_index,
                    protagonist_actor_index,
                    recipient_unit_index,
                    preselected_reply_actor_index: reply_actor_index,
                    protagonist_look_priority,
                    recipient_look_priority,
                    look_type,
                    look_unit_index,
                    sound_definition_index,
                    dialogue_type_index: this_index,
                });
                total_weight += weight;
            }
        }
    }

    if possibilities.is_empty() {
        return;
    }
    let mut selected = 0usize;
    if any_forced {
        total_weight = 0.0;
        for possibility in possibilities.iter_mut() {
            if !possibility.interrupts {
                possibility.weight = 0.0;
            }
            total_weight += possibility.weight;
        }
    }
    if possibilities.len() > 1 {
        let mut cumulative_weight = 0.0f32;
        let random_weight = call(call::RANDOM_REAL, 0, 0, &[]).output[0] * total_weight;
        let mut index = 0;
        while index < possibilities.len() - 1 {
            cumulative_weight += possibilities[index].weight;
            if cumulative_weight >= random_weight {
                break;
            }
            index += 1;
        }
        selected = index;
    }

    let selected = possibilities[selected];
    selected.write(m, context.communication_selected);
    let information = [bits(if information_type as i16 == NONE as i16 { INFORMATION_NONE as i32 } else { information_type })];
    call(
        if selected.is_reply { call::REPLY } else { call::SPEAK },
        communication_type,
        damage_type as i32,
        &information,
    );
}
