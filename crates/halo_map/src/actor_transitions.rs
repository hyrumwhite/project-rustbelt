//! actions.c's transition handlers, by which the actor types
//! ([`crate::actor_types`]) choose an actor's action: its initial action and
//! pending command list, what denies a transition, the panics and
//! berserking its stimuli set off, surprise, the panic transition, and
//! whether a guard or a conversation can stop.
//!
//! What they ask of the rest of the game goes through [`AiHost::call`]
//! (actions.c's `actions_port_handler_call`, shared with the C).

use crate::actions::dot;
use crate::actor_types::call as handler;
use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE, prop_get};
use crate::layout::{actor_datum as a, actor_definition as d, actor_variant_definition as var, ai_values as v, prop_datum as p};

/// What the handlers ask (halo_sim.h's `_actions_port_handler_call_*`).
pub mod call {
    const BASE: i32 = 0x1040;
    /// `actor_action_set_default_state(actor, NONE)`, the result.
    pub const SET_DEFAULT_STATE: i32 = BASE | 1;
    /// `action_obey_command_list_setup` of the command list `a` and, if it
    /// sets up, `actor_action_change` to obey: whether it did, the result.
    pub const OBEY_COMMAND_LIST: i32 = BASE | 2;
    /// Whether the game's local and `ai_debug.flee_always` is set, the
    /// result.
    pub const FLEE_ALWAYS: i32 = BASE | 3;
    /// `actor_perception_find_recent_damaging_prop_index(actor, TRUE)`, the
    /// result.
    pub const RECENT_DAMAGING_PROP: i32 = BASE | 4;
    /// The actor's prop of the unit responsible for its delayed attached
    /// projectile (or NONE), the result.
    pub const ATTACHED_PROJECTILE_PROP: i32 = BASE | 5;
    /// The actor's prop of the unit responsible for its unit's flaming
    /// death (or NONE), the result.
    pub const BURNING_PROP: i32 = BASE | 6;
    /// Whether the actor's squad's delay timer runs, the result.
    pub const SQUAD_DELAYED: i32 = BASE | 7;
    /// `encounter_squad_timer_expire` of the actor's squad.
    pub const SQUAD_TIMER_EXPIRE: i32 = BASE | 8;
    /// The flags of the conversation `a`'s definition, the result.
    pub const CONVERSATION_FLAGS: i32 = BASE | 9;
    /// `actor_move_animation_impulse(actor, a, input[0..2])`.
    pub const ANIMATION_IMPULSE: i32 = BASE | 10;
    /// `ai_communication_event(a, unit, b, input[0], NONE, NONE, NULL)`.
    pub const COMMUNICATION: i32 = BASE | 11;
    /// `actor_combat_fire_wildly(actor, a)`.
    pub const FIRE_WILDLY: i32 = BASE | 12;
    /// `actor_combat_disable_bursts(actor, a)`.
    pub const DISABLE_BURSTS: i32 = BASE | 13;
    /// `actor_stimulus_was_surprised(actor)`.
    pub const WAS_SURPRISED: i32 = BASE | 14;
    /// `actor_situation_try_new_target(actor, a)`.
    pub const TRY_NEW_TARGET: i32 = BASE | 15;
    /// `actor_combat_get_firing_variant_definition(actor)`, its address the
    /// result.
    pub const FIRING_VARIANT: i32 = BASE | 16;
    /// `real_seed_random(global seed)`, in `output[0]`.
    pub const RANDOM: i32 = BASE | 17;
    /// `actor_action_try_to_panic(actor, a, b, input[0])`, the result.
    pub const TRY_TO_PANIC: i32 = BASE | 18;
    /// `actor_action_handle_combat_selection(actor)`, the result.
    pub const COMBAT_SELECTION: i32 = BASE | 19;
    /// `actor_action_handle_lost_contact(actor)`, the result.
    pub const LOST_CONTACT: i32 = BASE | 20;
    /// `action_guard_setup_from_fleeing` and `actor_action_change` to guard.
    pub const GUARD_FROM_FLEEING: i32 = BASE | 21;
    /// `actor_berserk(actor, TRUE)`.
    pub const BERSERK: i32 = BASE | 22;
    /// `action_guard_setup_from_combat_transition` and, if it sets up,
    /// `actor_action_change` to guard: whether it did, the result.
    pub const GUARD_FROM_COMBAT_TRANSITION: i32 = BASE | 23;
    /// `actor_perception_tried_to_uncover(actor, a)`.
    pub const TRIED_TO_UNCOVER: i32 = BASE | 24;
    /// `actor_perception_tried_to_search(actor, a)`.
    pub const TRIED_TO_SEARCH: i32 = BASE | 25;
    /// `actor_perception_abandoned_search(actor, a)`.
    pub const ABANDONED_SEARCH: i32 = BASE | 26;
    /// `actor_action_try_to_seek_cover(actor, a, b)`, the result.
    pub const TRY_TO_SEEK_COVER: i32 = BASE | 27;
    /// `actor_has_ranged_weapon(actor)`, the result.
    pub const HAS_RANGED_WEAPON: i32 = BASE | 28;
    /// The game's difficulty value `a` (the melee delay's scale, 0, or its
    /// base, 1), in `output[0]`.
    pub const DIFFICULTY: i32 = BASE | 29;
    /// `action_charge_setup` for the goal `a` and, if it sets up,
    /// `actor_action_change` to charge: whether it did, the result.
    pub const CHARGE: i32 = BASE | 30;
    /// The definition of the actor's vehicle, its address the result.
    pub const VEHICLE_DEFINITION: i32 = BASE | 31;
    /// `action_fight_setup` and `actor_action_change` to fight.
    pub const FIGHT: i32 = BASE | 32;
}

/// The difficulty values [`call::DIFFICULTY`] reads.
mod difficulty {
    pub const MELEE_DELAY_SCALE: i32 = 0;
    pub const MELEE_DELAY_BASE: i32 = 1;
}

/// actions.c's vehicle definition's AI fields (its local layout).
const VEHICLE_AI_CHARGE_REPEAT_TIME: u32 = 0x390;
const VEHICLE_AI_STRAFING_STOP_RANGE: u32 = 0x394;

/// The actions (actions.h's `_actor_action_*`).
const ACTION_NONE: i16 = 0;
const ACTION_ALERT: i16 = 2;
const ACTION_FIGHT: i16 = 3;
const ACTION_FLEE: i16 = 4;
const ACTION_UNCOVER: i16 = 5;
const ACTION_SEARCH: i16 = 7;
const ACTION_WAIT: i16 = 8;
const ACTION_CHARGE: i16 = 10;
const ACTION_OBEY: i16 = 11;

/// Each action's class (actions.c's `global_action_functions`).
const ACTION_CLASSES: [i16; 14] = [0, 0, 0, 4, 2, 3, 1, 3, 3, 2, 4, 2, 2, 2];

/// actions.c's panics (`_actor_panic_*`).
const PANIC_NONE: i16 = 0;
const PANIC_DAMAGE: i16 = 1;
const PANIC_SURPRISE: i16 = 7;
const PANIC_DELAYED_PROJECTILE_ATTACHED_TO_US: i16 = 10;
const PANIC_MELEE_ATTACHED_TO_US: i16 = 11;
const PANIC_BURNING_TO_DEATH: i16 = 12;

/// actions.c's berserking (`_actor_berserk_*`).
const BERSERK_ATTACKING_MODE: i16 = 1;
const BERSERK_PROXIMITY: i16 = 2;
const BERSERK_DAMAGE: i16 = 3;
const BERSERK_ATTACHED_PROJECTILE: i16 = 4;

/// actions.c's conversation flags (`_ai_conversation_stop_if_*_bit`).
const CONVERSATION_STOP_IF_DAMAGED_BIT: u32 = 1;
const CONVERSATION_STOP_IF_VISIBLE_ENEMY_BIT: u32 = 2;
const CONVERSATION_STOP_IF_ALERTED_TO_ENEMY_BIT: u32 = 3;

/// actions.c's surprise animations (`_actor_animation_surprise_*`).
const ANIMATION_SURPRISE_FRONT: i32 = 4;
const ANIMATION_SURPRISE_BACK: i32 = 5;

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32, input: &[f32]) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    call.input[..input.len()].copy_from_slice(input);
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0, &[]).result
}

fn normalize2(v: &mut [f32; 2]) -> f32 {
    let magnitude = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if !(0.0001 > magnitude.abs()) {
        let s = 1.0 / magnitude;
        *v = [s * v[0], s * v[1]];
        magnitude
    } else {
        0.0
    }
}

/// The handlers ported, by the types' call ([`crate::actor_types::call`]):
/// what one returns, or None for a handler that's still the game's.
pub fn run(kind: i32, a: i32, b: i32, c: i32, context: &AiStepContext, host: &impl AiHost) -> Option<bool> {
    Some(match kind {
        handler::INITIAL_ACTION => handle_initial_action(context, host),
        handler::PENDING_COMMAND_LIST => handle_pending_command_list(context, host),
        handler::SURPRISE => handle_surprise(context, host, a as i16),
        handler::DENY_TRANSITION => deny_transition(context, host),
        handler::PANIC_FROM_SURPRISE => handle_panic_from_surprise(context, host),
        handler::PANIC_FROM_DAMAGE => handle_panic_from_damage(context, host),
        handler::PANIC_FROM_ATTACHED_PROJECTILES => handle_panic_from_attached_projectiles(context, host),
        handler::PANIC_FROM_ATTACHED_MELEE_ATTACKERS => handle_panic_from_attached_melee_attackers(context, host),
        handler::PANIC_FROM_BURNING_TO_DEATH => handle_panic_from_burning_to_death(context, host),
        handler::PANIC_TRANSITION => handle_panic_transition(context, host, a as i16, b != 0, c as i16),
        handler::CAN_STOP_GUARDING => can_stop_guarding(context, host, a as i16, b as i16),
        handler::CAN_STOP_CONVERSING => can_stop_conversing(context, host),
        handler::BERSERKING_FROM_ATTACKING_MODE => handle_berserking_from_attacking_mode(context, host),
        handler::BERSERKING_FROM_DAMAGE => handle_berserking_from_damage(context, host),
        handler::BERSERKING_FROM_PROXIMITY => handle_berserking_from_proximity(context, host),
        handler::BERSERKING_FROM_ATTACHED_PROJECTILES => handle_berserking_from_attached_projectiles(context, host),
        handler::DONE_FLEEING => handle_done_fleeing(context, host),
        handler::COMBAT_STATUS => handle_combat_status(context, host, a != 0, b != 0),
        handler::COMBAT_FAILURE => handle_combat_failure(context, host),
        handler::BERSERK_TRANSITION => handle_berserk_transition(context, host, a as i16),
        handler::COMBAT_TRANSITION => handle_combat_transition(context, host),
        handler::EXIT_PURSUIT => handle_exit_pursuit(context, host),
        _ => return None,
    })
}

/// `actor_action_handle_initial_action`: an actor with no action takes its
/// default (unless it's asleep).
pub fn handle_initial_action(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_ACTION)) == ACTION_NONE && m.i16(at(a::STATE_MODE)) != 0 {
        call_simple(context, host, call::SET_DEFAULT_STATE, 0) != 0
    } else {
        false
    }
}

/// `actor_action_handle_pending_command_list`: a command list given the
/// actor starts obeying (unless the transition's denied or the actor's
/// asleep, for a list that isn't immediate).
pub fn handle_pending_command_list(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut entered_obey = false;
    let command_list_index = m.i16(at(a::STATE_COMMAND_LIST_INDEX));
    if command_list_index != NONE as i16 {
        let blocked = !m.bool(at(a::STATE_COMMAND_LIST_IMMEDIATE))
            && (m.i16(at(a::STATE_MODE)) == 0 || deny_transition(context, host));
        if !blocked {
            let command_list_index = m.i16(at(a::STATE_COMMAND_LIST_INDEX));
            if call_simple(context, host, call::OBEY_COMMAND_LIST, command_list_index as i32) != 0 {
                entered_obey = true;
            }
            m.set_bool(at(a::STATE_COMMAND_LIST_IMMEDIATE), false);
            m.set_i16(at(a::STATE_COMMAND_LIST_INDEX), NONE as i16);
        }
    }
    entered_obey
}

/// Raises the actor's panic to `panic_type`, taking `panic_prop_index` as
/// what it panics from when it had no panic or no prop.
fn raise_panic(context: &AiStepContext, host: &impl AiHost, panic_type: i16, panic_prop_index: i32) {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STIMULI_PANIC_TYPE)) == PANIC_NONE || m.i32(at(a::STIMULI_PANIC_PROP_INDEX)) == NONE {
        m.w32(at(a::STIMULI_PANIC_PROP_INDEX), panic_prop_index as u32);
    }
    m.set_i16(at(a::STIMULI_PANIC_TYPE), m.i16(at(a::STIMULI_PANIC_TYPE)).max(panic_type));
}

/// `actor_action_handle_panic_from_attached_melee_attackers`.
pub fn handle_panic_from_attached_melee_attackers(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_MELEE_ATTACKER_ATTACHED)) {
        m.set_i16(at(a::STIMULI_PANIC_TYPE), m.i16(at(a::STIMULI_PANIC_TYPE)).max(PANIC_MELEE_ATTACHED_TO_US));
        m.w32(at(a::STIMULI_PANIC_PROP_INDEX), NONE as u32);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_surprise`.
pub fn handle_panic_from_surprise(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let flags = m.r32(context.definition + d::FLAGS as u32);
    if m.bool(at(a::STIMULI_WAS_SURPRISED)) && flags & (1 << v::_ACTOR_DEFINITION_PANIC_ON_SURPRISE_BIT) != 0 {
        raise_panic(context, host, PANIC_SURPRISE, m.i32(at(a::STIMULI_SURPRISE_PROP_INDEX)));
        m.set_bool(at(a::STIMULI_WAS_SURPRISED), false);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_damage`.
pub fn handle_panic_from_damage(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::STIMULI_WAS_DAMAGED))
        && (call_simple(context, host, call::FLEE_ALWAYS, 0) != 0
            || m.f32(at(a::INPUT_RECENT_BODY_DAMAGE)) > m.f32(context.definition + d::PANIC_PANIC_DAMAGE_THRESHOLD as u32))
    {
        if m.i16(at(a::STIMULI_PANIC_TYPE)) == PANIC_NONE || m.i32(at(a::STIMULI_PANIC_PROP_INDEX)) == NONE {
            let prop_index = call_simple(context, host, call::RECENT_DAMAGING_PROP, 0);
            m.w32(at(a::STIMULI_PANIC_PROP_INDEX), prop_index as u32);
        }
        m.set_i16(at(a::STIMULI_PANIC_TYPE), m.i16(at(a::STIMULI_PANIC_TYPE)).max(PANIC_DAMAGE));
        m.set_bool(at(a::STIMULI_WAS_DAMAGED), false);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_attached_projectiles`.
pub fn handle_panic_from_attached_projectiles(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    if m.i32(context.actor + a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX as u32) != NONE {
        let prop_index = call_simple(context, host, call::ATTACHED_PROJECTILE_PROP, 0);
        raise_panic(context, host, PANIC_DELAYED_PROJECTILE_ATTACHED_TO_US, prop_index);
        true
    } else {
        false
    }
}

/// `actor_action_handle_panic_from_burning_to_death`.
pub fn handle_panic_from_burning_to_death(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.bool(at(a::INPUT_BURNING_TO_DEATH)) {
        let mut prop_index = NONE;
        if m.i32(at(a::META_UNIT_INDEX)) != NONE {
            prop_index = call_simple(context, host, call::BURNING_PROP, 0);
        }
        raise_panic(context, host, PANIC_BURNING_TO_DEATH, prop_index);
        true
    } else {
        false
    }
}

/// Raises the actor's berserking to `berserk_type`.
fn raise_berserk(context: &AiStepContext, host: &impl AiHost, berserk_type: i16) {
    let m = host.memory();
    let address = context.actor + a::STIMULI_BERSERK_TYPE as u32;
    m.set_i16(address, m.i16(address).max(berserk_type));
}

/// `actor_action_handle_berserking_from_attacking_mode`.
pub fn handle_berserking_from_attacking_mode(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let flags = m.r32(context.definition + d::FLAGS as u32);
    if flags & (1 << v::_ACTOR_DEFINITION_ALWAYS_BERSERK_IN_ATTACKING_MODE_BIT) != 0
        && !m.bool(at(a::EXTERNAL_ORDERS_DEFENDING))
        && m.i16(at(a::STATE_COMBAT_STATUS)) >= 5
    {
        raise_berserk(context, host, BERSERK_ATTACKING_MODE);
        true
    } else {
        false
    }
}

/// `actor_action_handle_berserking_from_damage`.
pub fn handle_berserking_from_damage(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| m.f32(context.definition + offset as u32);
    if m.bool(at(a::STIMULI_WAS_DAMAGED))
        && m.f32(at(a::INPUT_RECENT_BODY_DAMAGE)) > definition(d::BERSERK_DAMAGE_BERSERK_AMOUNT)
        && m.f32(at(a::INPUT_BODY_VITALITY)) < definition(d::BERSERK_DAMAGE_BERSERK_THRESHOLD)
    {
        raise_berserk(context, host, BERSERK_DAMAGE);
        m.set_bool(at(a::STIMULI_WAS_DAMAGED), false);
        true
    } else {
        false
    }
}

/// `actor_action_handle_berserking_from_proximity`.
pub fn handle_berserking_from_proximity(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
        let prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
        if m.f32(prop + p::DISTANCE as u32) < m.f32(context.definition + d::BERSERK_PROXIMITY_BERSERK_DISTANCE as u32) {
            raise_berserk(context, host, BERSERK_PROXIMITY);
            return true;
        }
    }
    false
}

/// `actor_action_handle_berserking_from_attached_projectiles`.
pub fn handle_berserking_from_attached_projectiles(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let firing_variant = call_simple(context, host, call::FIRING_VARIANT, 0) as u32;
    if m.i32(at(a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX)) != NONE && m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
        let prop = prop_get(m, context, m.i32(at(a::TARGET_TARGET_PROP_INDEX)));
        if m.f32(prop + p::DISTANCE as u32) < m.f32(firing_variant + var::RANGED_COMBAT_BERSERK_FIRING_RANGE_UPPER_BOUND as u32)
            && call_full(context, host, call::RANDOM, 0, 0, &[]).output[0]
                < m.f32(context.definition + d::BERSERK_GRENADE_ATTACHED_CHANCE as u32)
        {
            raise_berserk(context, host, BERSERK_ATTACHED_PROJECTILE);
            return true;
        }
    }
    false
}

/// `actor_action_deny_transition`: an actor waiting on its command list's
/// delay or its squad's (unless it's in combat, which ends the squad's), or
/// obeying without initiative, keeps its action.
pub fn deny_transition(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut deny = false;
    if m.i16(at(a::STATE_COMMAND_LIST_INDEX)) != NONE as i16 && m.i16(at(a::STATE_COMMAND_LIST_DELAY_TIMER)) > 0 {
        deny = true;
    }
    if m.i32(at(a::META_ENCOUNTER_INDEX)) != NONE && call_simple(context, host, call::SQUAD_DELAYED, 0) != 0 {
        if m.i16(at(a::STATE_COMBAT_STATUS)) >= 5 {
            call_simple(context, host, call::SQUAD_TIMER_EXPIRE, 0);
        } else {
            deny = true;
        }
    }
    if m.i16(at(a::STATE_ACTION)) == ACTION_OBEY
        && !m.bool(at(a::STATE_OBEY_INITIATIVE))
        && !m.bool(at(a::STATE_OBEY_FINISHED))
    {
        deny = true;
    }
    deny
}

/// `actor_action_can_stop_guarding`.
pub fn can_stop_guarding(
    context: &AiStepContext,
    host: &impl AiHost,
    guard_investigate_threshold: i16,
    cower_investigate_threshold: i16,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let combat_status = m.i16(at(a::STATE_COMBAT_STATUS));
    if m.bool(at(a::STATE_GUARD_COWER)) {
        return combat_status >= cower_investigate_threshold;
    }
    if m.i16(at(a::STATE_GUARD_WAIT_TICKS)) <= 0 {
        return true;
    }
    combat_status >= guard_investigate_threshold
        || (m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) > 0 && !m.bool(at(a::STATE_GUARD_POST_COMBAT)))
}

/// `actor_action_can_stop_conversing`: by the conversation's flags, when
/// damaged, seeing an enemy or alerted to one.
pub fn can_stop_conversing(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let conversation_index = m.i32(at(a::EXTERNAL_ORDERS_CONVERSATION_INDEX));
    if conversation_index == NONE {
        return true;
    }
    let flags = call_simple(context, host, call::CONVERSATION_FLAGS, conversation_index) as u32 & 0xFFFF;
    let target_type = m.i16(at(a::TARGET_TARGET_TYPE));
    (flags & (1 << CONVERSATION_STOP_IF_DAMAGED_BIT) != 0 && m.i8(at(a::SITUATION_SPECIFIC_THREATS) + 8) != 0)
        || (flags & (1 << CONVERSATION_STOP_IF_VISIBLE_ENEMY_BIT) != 0 && target_type >= 9)
        || (flags & (1 << CONVERSATION_STOP_IF_ALERTED_TO_ENEMY_BIT) != 0 && target_type >= 6)
}

/// `actor_action_handle_surprise`: an actor surprised enough (and not a
/// passenger) flinches toward or away from what surprised it, shouts, fires
/// wildly or holds its fire as its variant says, and takes what surprised
/// it as a target; its surprise then clears.
pub fn handle_surprise(context: &AiStepContext, host: &impl AiHost, surprise_level: i16) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut result = false;
    if !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) && m.i16(at(a::STIMULI_SURPRISE_LEVEL)) >= surprise_level {
        let (direction, animation) = if m.bool(at(a::STIMULI_SURPRISE_HAS_VECTOR)) {
            let vector = m.vector(at(a::STIMULI_SURPRISE_VECTOR));
            let mut direction = [vector[0], vector[1]];
            normalize2(&mut direction);
            let facing = m.vector(at(a::CONTROL_DESIRED_FACING_VECTOR));
            if !(direction[1] * facing[1] + direction[0] * facing[0] >= 0.0) {
                ([-direction[0], -direction[1]], ANIMATION_SURPRISE_BACK)
            } else {
                (direction, ANIMATION_SURPRISE_FRONT)
            }
        } else {
            let facing = m.vector(at(a::INPUT_FACING_VECTOR));
            let mut direction = [facing[0], facing[1]];
            normalize2(&mut direction);
            (direction, ANIMATION_SURPRISE_FRONT)
        };
        call_full(context, host, call::ANIMATION_IMPULSE, animation, 0, &direction);

        let mut unit_index = NONE;
        let mut weapon_state = 0;
        let surprise_prop_index = m.i32(at(a::STIMULI_SURPRISE_PROP_INDEX));
        if surprise_prop_index != NONE {
            let prop = prop_get(m, context, surprise_prop_index);
            unit_index = m.i32(prop + p::UNIT_INDEX as u32);
            weapon_state = m.bool(prop + p::ENEMY as u32) as i32 + 2;
        }
        call_full(
            context,
            host,
            call::COMMUNICATION,
            v::_AI_COMMUNICATION_SURPRISE as i32,
            unit_index,
            &[weapon_state as f32],
        );

        let variant = |offset: usize| m.f32(context.variant_definition + offset as u32);
        let wildfire_time = variant(var::RANGED_COMBAT_SURPRISE_WILDFIRE_TIME);
        if wildfire_time > 0.0 {
            call_simple(context, host, call::FIRE_WILDLY, (wildfire_time * 30.0) as i32);
        }
        let delay_time = variant(var::RANGED_COMBAT_SURPRISE_DELAY_TIME);
        if delay_time > 0.0 {
            call_simple(context, host, call::DISABLE_BURSTS, (delay_time * 30.0) as i32);
        }
        call_simple(context, host, call::WAS_SURPRISED, 0);
        let surprise_prop_index = m.i32(at(a::STIMULI_SURPRISE_PROP_INDEX));
        if surprise_prop_index != NONE {
            call_simple(context, host, call::TRY_NEW_TARGET, surprise_prop_index);
        }
        result = true;
    }
    m.set_i16(at(a::STIMULI_SURPRISE_LEVEL), 0);
    result
}

/// `actor_action_handle_panic_transition`: a panic of at least
/// `minimum_panic_type` (not in a passenger) raises a fleeing actor's panic,
/// or (unless a flee just failed) makes it panic; one below
/// `force_panic_type` only shouts that it wants to when
/// `communicate_failure`. The actor's panic then clears.
pub fn handle_panic_transition(
    context: &AiStepContext,
    host: &impl AiHost,
    minimum_panic_type: i16,
    communicate_failure: bool,
    force_panic_type: i16,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let panic_type = m.i16(at(a::STIMULI_PANIC_TYPE));
    let mut result = false;
    'done: {
        if panic_type >= minimum_panic_type && !m.bool(at(a::INPUT_VEHICLE_PASSENGER)) {
            let flee_panic_type = m.i16(at(a::STATE_FLEE_PANIC_TYPE));
            if m.i16(at(a::STATE_ACTION)) == ACTION_FLEE && flee_panic_type > PANIC_NONE {
                m.set_i16(at(a::STATE_FLEE_PANIC_TYPE), flee_panic_type.max(panic_type));
                break 'done;
            }
            let last_flee_failed_time = m.i32(at(a::EMOTIONS_LAST_FLEE_FAILED_TIME));
            if last_flee_failed_time != NONE && last_flee_failed_time.wrapping_add(7) >= context.game_time {
                break 'done;
            }
            let force = m.i16(at(a::STIMULI_PANIC_TYPE)) >= force_panic_type;
            if communicate_failure && !force {
                call_full(
                    context,
                    host,
                    call::COMMUNICATION,
                    v::_AI_COMMUNICATION_ATTEMPTED_FLEE as i32,
                    NONE,
                    &[NONE as f32],
                );
                break 'done;
            }
            result = call_full(
                context,
                host,
                call::TRY_TO_PANIC,
                m.i16(at(a::STIMULI_PANIC_TYPE)) as i32,
                m.i32(at(a::STIMULI_PANIC_PROP_INDEX)),
                &[force as i32 as f32],
            )
            .result
                != 0;
        }
    }
    m.set_i16(at(a::STIMULI_PANIC_TYPE), PANIC_NONE);
    result
}

fn combat_selection(context: &AiStepContext, host: &impl AiHost) -> bool {
    handle_combat_selection(context, host)
}

fn lost_contact(context: &AiStepContext, host: &impl AiHost) -> bool {
    call_simple(context, host, call::LOST_CONTACT, 0) != 0
}

/// `actor_action_handle_done_fleeing`: a fled actor guards.
pub fn handle_done_fleeing(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_ACTION)) == ACTION_FLEE && m.bool(at(a::STATE_FLEE_DONE_FLEEING)) {
        call_simple(context, host, call::GUARD_FROM_FLEEING, 0);
        true
    } else {
        false
    }
}

/// `actor_action_handle_combat_status`: by the action's class, a new
/// combat action when the actor can take the initiative (or must decide) and
/// is in combat, or the lost-contact decision when it isn't (or a pursuit's
/// target changed or is yet to be uncovered or searched).
pub fn handle_combat_status(
    context: &AiStepContext,
    host: &impl AiHost,
    allow_initiative: bool,
    force_decision: bool,
) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let action_class = ACTION_CLASSES[m.i16(at(a::STATE_ACTION)) as usize];
    let allow_initiative = allow_initiative || force_decision;
    let combat_status = || m.i16(at(a::STATE_COMBAT_STATUS));
    let mut result = false;

    match action_class {
        1 | 2 => {
            if allow_initiative {
                if combat_status() >= 5 {
                    result = combat_selection(context, host);
                } else if combat_status() >= 2
                    || m.i16(at(a::STATE_ACTION)) == ACTION_ALERT
                    || (combat_status() == 0
                        && (m.bool(at(a::EXTERNAL_ORDERS_STAND_DOWN))
                            || m.i16(at(a::EXTERNAL_ORDERS_POSTCOMBAT_TYPE)) > 0))
                {
                    result = lost_contact(context, host);
                }
            }
        }
        4 => {
            result = if combat_status() >= 4 { combat_selection(context, host) } else { lost_contact(context, host) };
        }
        3 => {
            if allow_initiative && combat_status() >= 4 {
                result = combat_selection(context, host);
            } else if combat_status() < 2 {
                result = lost_contact(context, host);
            } else {
                let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
                if target_prop_index != NONE {
                    let prop = prop_get(m, context, target_prop_index);
                    let action = m.i16(at(a::STATE_ACTION));
                    // the game's reads the search's pursuit location for an
                    // uncover too (the two share it)
                    let pursuit_location_type = m.i16(at(a::STATE_SEARCH_PURSUIT_LOCATION_TYPE));
                    if target_prop_index != m.i32(at(a::FIRING_POSITIONS_PURSUIT_PROP_INDEX))
                        || (!m.bool(prop + p::TRIED_TO_UNCOVER as u32)
                            && (action != ACTION_UNCOVER || pursuit_location_type != 0))
                        || (!m.bool(prop + p::TRIED_TO_SEARCH as u32)
                            && (action != ACTION_UNCOVER || pursuit_location_type != 0)
                            && (action != ACTION_SEARCH || pursuit_location_type != 0))
                    {
                        result = lost_contact(context, host);
                    }
                }
            }
        }
        _ => {}
    }

    if !result && force_decision {
        result = lost_contact(context, host);
    }
    result
}

/// `actor_action_handle_combat_failure`: a charge's melee attack that ended
/// or couldn't advance, or a vehicle's charge that couldn't, chooses again.
pub fn handle_combat_failure(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_ACTION)) != ACTION_CHARGE {
        return false;
    }
    let goal = m.i16(at(a::STATE_CHARGE_GOAL));
    let failed = if goal == v::_CHARGE_GOAL_MELEE as i16 || goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16 {
        m.bool(at(a::STATE_CHARGE_FINISHED_MELEE_ATTACK))
            || m.bool(at(a::STATE_CHARGE_ABORTED_MELEE_ATTACK))
            || m.bool(at(a::STATE_CHARGE_UNABLE_TO_ADVANCE))
    } else if goal == v::_CHARGE_GOAL_VEHICLE_STRAFING as i16 || goal == v::_CHARGE_GOAL_VEHICLE_RAMMING as i16 {
        m.bool(at(a::STATE_CHARGE_UNABLE_TO_ADVANCE))
    } else {
        false
    };
    failed && combat_selection(context, host)
}

/// `actor_action_handle_berserk_transition`: berserking of at least
/// `berserk_level` sends the actor berserk (choosing a combat action if it's
/// in combat); its berserking then clears.
pub fn handle_berserk_transition(context: &AiStepContext, host: &impl AiHost, berserk_level: i16) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let mut result = false;
    if m.i16(at(a::STIMULI_BERSERK_TYPE)) >= berserk_level && !m.bool(at(a::EMOTIONS_BERSERK)) {
        call_simple(context, host, call::BERSERK, 0);
        if m.i16(at(a::STATE_COMBAT_STATUS)) >= 4 {
            result = combat_selection(context, host);
        }
    }
    m.set_i16(at(a::STIMULI_BERSERK_TYPE), 0);
    result
}

/// `actor_action_handle_combat_transition`: an actor going into combat
/// guards (or chooses a combat action).
pub fn handle_combat_transition(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    if m.i16(at(a::STATE_MODE)) < 3 && m.i16(at(a::STIMULI_COMBAT_TRANSITION)) != 0 {
        m.set_i16(at(a::STATE_MODE), 3);
        if call_simple(context, host, call::GUARD_FROM_COMBAT_TRANSITION, 0) == 0 {
            combat_selection(context, host);
        }
        m.set_i16(at(a::STIMULI_COMBAT_TRANSITION), 0);
        true
    } else {
        false
    }
}

/// `actor_action_handle_exit_pursuit`: a finished uncover, search or wait
/// reports it to the target's prop and takes the lost-contact decision.
pub fn handle_exit_pursuit(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let target_prop_index = || m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    let mut reported = false;
    match m.i16(at(a::STATE_ACTION)) {
        ACTION_UNCOVER => {
            if m.bool(at(a::STATE_UNCOVER_UNCOVER_DONE)) {
                if m.i16(at(a::STATE_UNCOVER_PURSUIT_LOCATION_TYPE)) == 0 {
                    call_simple(context, host, call::TRIED_TO_UNCOVER, target_prop_index());
                }
                reported = true;
            }
        }
        ACTION_SEARCH => {
            if m.bool(at(a::STATE_SEARCH_SEARCH_DONE)) {
                if m.i16(at(a::STATE_SEARCH_PURSUIT_LOCATION_TYPE)) == 0 {
                    call_simple(context, host, call::TRIED_TO_SEARCH, target_prop_index());
                }
                reported = true;
            }
        }
        ACTION_WAIT => {
            if m.bool(at(a::STATE_WAIT_WAIT_DONE)) {
                call_simple(context, host, call::ABANDONED_SEARCH, target_prop_index());
                reported = true;
            }
        }
        _ => {}
    }
    reported && lost_contact(context, host)
}

fn c_short(value: f32) -> i16 {
    let long = if (-2_147_483_648.0..2_147_483_648.0).contains(&value) { value as i32 } else { i32::MIN };
    long as i16
}

/// `actor_action_handle_combat_selection`: the actor's combat action. A
/// discovered stalker seeks cover or goes berserk; an actor near enough a
/// target (on foot, or with no gun) charges it to melee, as often as its
/// melee delay allows; a flying driver strafes a target it can't see; and
/// otherwise it charges when it wants to (forced, gunless, or still charging)
/// or fights.
pub fn handle_combat_selection(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let at = |offset: usize| context.actor + offset as u32;
    let definition = |offset: usize| context.definition + offset as u32;
    let variant = |offset: usize| context.variant_definition + offset as u32;
    let firing_variant = call_simple(context, host, call::FIRING_VARIANT, 0) as u32;
    let has_ranged_weapon = || call_simple(context, host, call::HAS_RANGED_WEAPON, 0) != 0;
    let action = || m.i16(at(a::STATE_ACTION));
    let goal = || m.i16(at(a::STATE_CHARGE_GOAL));
    let goal_melee = |goal: i16| goal == v::_CHARGE_GOAL_MELEE as i16 || goal == v::_CHARGE_GOAL_MELEE_LEAPING as i16;
    let definition_flag = |bit: usize| m.r32(definition(d::FLAGS)) & (1 << bit) != 0;
    let mut action_changed = false;
    let mut prop = 0;
    let mut prop_distance = f32::MAX;

    let target_prop_index = m.i32(at(a::TARGET_TARGET_PROP_INDEX));
    if target_prop_index != NONE {
        prop = prop_get(m, context, target_prop_index);
        prop_distance = m.f32(prop + p::DISTANCE as u32);

        if action() == ACTION_CHARGE && goal() == v::_CHARGE_GOAL_STALKING as i16 {
            let discovery_time = m.f32(definition(d::DEFENSIVE_STALKING_DISCOVERY_TIME));
            let discovered = m.bool(prop + p::CURRENTLY_DAMAGING_ME as u32)
                || (m.bool(prop + p::SHOOTING as u32) && m.i8(prop + p::QUANTIZED_DISTANCE as u32) <= 1)
                || (discovery_time > 0.0
                    && m.i16(at(a::STATE_CHARGE_STALKING_DISCOVERY_TIMER)) >= c_short(discovery_time * 30.0));
            if discovered {
                if prop_distance > m.f32(firing_variant + var::RANGED_COMBAT_COMBAT_RANGE_UPPER_BOUND as u32) {
                    action_changed = call_full(context, host, call::TRY_TO_SEEK_COVER, 0, 0, &[]).result != 0;
                }
                if !action_changed {
                    call_simple(context, host, call::BERSERK, 0);
                }
            }
        }

        if (!has_ranged_weapon() || (m.i32(prop + p::VEHICLE_INDEX as u32) == NONE && !m.bool(prop + p::SWARM as u32)))
            && (action() != ACTION_CHARGE || !goal_melee(goal()))
            && !action_changed
            && !m.bool(at(a::META_SWARM))
            && m.i32(at(a::INPUT_VEHICLE_INDEX)) == NONE
            && m.i16(at(a::CONTROL_FIRE_STATE)) != v::_ACTOR_FIRE_STATE_BURSTING as i16
        {
            let current_time = context.game_time;
            let berserk = m.bool(at(a::EMOTIONS_BERSERK));
            let mut use_berserk_range = berserk;
            if !has_ranged_weapon() && !definition_flag(v::_ACTOR_DEFINITION_STALKING_BEHAVIOR_BIT) {
                use_berserk_range = true;
            }
            let charge_delay = if berserk { 0.0 } else { m.f32(definition(d::BERSERK_MELEE_ATTACK_DELAY_TIMER)) };
            let scale = call_full(context, host, call::DIFFICULTY, difficulty::MELEE_DELAY_SCALE, 0, &[]).output[0];
            let base = call_full(context, host, call::DIFFICULTY, difficulty::MELEE_DELAY_BASE, 0, &[]).output[0];
            let charge_delay = scale * charge_delay + base;
            let charge_range = m.f32(variant(if use_berserk_range {
                var::RANGED_COMBAT_BERSERK_MELEE_RANGE
            } else {
                var::RANGED_COMBAT_MELEE_RANGE
            }));
            let mut begin_charge = false;

            let last_melee_check_time = m.i32(at(a::EMOTIONS_LAST_MELEE_CHECK_TIME));
            if (last_melee_check_time == NONE || last_melee_check_time.wrapping_add(10) < current_time)
                && !(prop_distance > charge_range)
            {
                let mut in_range = true;
                if m.bool(at(a::EXTERNAL_ORDERS_DISABLE_CHARGING)) {
                    let melee_attack_range = m.f32(definition(d::BERSERK_MELEE_ATTACK_RANGE));
                    let point_blank_range = if 0.0 > melee_attack_range { 0.0 } else { melee_attack_range };
                    if prop_distance > point_blank_range + 0.8 {
                        in_range = false;
                    }
                }
                if in_range {
                    let last_charge_time = m.i32(at(a::EMOTIONS_LAST_MELEE_ATTACK_TIME));
                    if last_charge_time == NONE || !(charge_delay * 30.0 + last_charge_time as f32 >= current_time as f32) {
                        // the game's asks again, the answer unused
                        has_ranged_weapon();
                        begin_charge = true;
                    }
                }
            }

            if begin_charge {
                m.w32(at(a::EMOTIONS_LAST_MELEE_CHECK_TIME), current_time as u32);
                if call_simple(context, host, call::CHARGE, v::_CHARGE_GOAL_MELEE as i32) != 0 {
                    action_changed = true;
                }
            }
        }

        let driver_type = m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE));
        if action() != ACTION_CHARGE
            && !m.bool(at(a::EXTERNAL_ORDERS_DISABLE_CHARGING))
            && !action_changed
            && driver_type > v::_ACTOR_VEHICLE_DRIVER_NONE as i16
        {
            let vehicle_definition = call_simple(context, host, call::VEHICLE_DEFINITION, 0) as u32;
            let last_charge_time = m.i32(at(a::EMOTIONS_LAST_VEHICLE_CHARGE_TIME));
            let repeat_elapsed = last_charge_time == NONE
                || m.f32(vehicle_definition + VEHICLE_AI_CHARGE_REPEAT_TIME) * 30.0 + (last_charge_time as f32)
                    < context.game_time as f32;
            if repeat_elapsed {
                let begin_charge = driver_type == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16
                    && prop_distance > m.f32(firing_variant + var::RANGED_COMBAT_MELEE_RANGE as u32)
                    && m.i16(prop + p::LINE_OF_SIGHT as u32) == 0;
                if begin_charge
                    && call_simple(context, host, call::CHARGE, v::_CHARGE_GOAL_VEHICLE_STRAFING as i32) != 0
                {
                    action_changed = true;
                }
            }
        }
    }

    if !action_changed {
        let disable_charging = || m.bool(at(a::EXTERNAL_ORDERS_DISABLE_CHARGING));
        let mut desires_charge = m.bool(at(a::EMOTIONS_FORCED_TO_CHARGE)) && !disable_charging();
        let mut restart_charge = false;

        if !disable_charging()
            && !has_ranged_weapon()
            && definition_flag(v::_ACTOR_DEFINITION_CHARGE_IN_ATTACKING_MODE_BIT)
        {
            desires_charge = true;
        }

        if action() == ACTION_CHARGE {
            let goal = goal();
            let unable_to_advance = m.bool(at(a::STATE_CHARGE_UNABLE_TO_ADVANCE));
            if goal_melee(goal) {
                if !m.bool(at(a::STATE_CHARGE_FINISHED_MELEE_ATTACK))
                    && !m.bool(at(a::STATE_CHARGE_ABORTED_MELEE_ATTACK))
                    && !unable_to_advance
                {
                    desires_charge = true;
                } else {
                    restart_charge = true;
                }
            } else if disable_charging() {
                desires_charge = false;
                restart_charge = true;
            } else if goal == v::_CHARGE_GOAL_VEHICLE_STRAFING as i16 || goal == v::_CHARGE_GOAL_VEHICLE_RAMMING as i16 {
                if unable_to_advance || m.i16(at(a::INPUT_VEHICLE_DRIVER_TYPE)) <= v::_ACTOR_VEHICLE_DRIVER_UNKNOWN as i16 {
                    restart_charge = true;
                } else {
                    desires_charge = true;
                    if goal == v::_CHARGE_GOAL_VEHICLE_STRAFING as i16 {
                        let vehicle_definition = call_simple(context, host, call::VEHICLE_DEFINITION, 0) as u32;
                        let stop_range = m.f32(vehicle_definition + VEHICLE_AI_STRAFING_STOP_RANGE);
                        if m.bool(at(a::CONTROL_PATH_AT_DESTINATION))
                            && m.i16(at(a::CONTROL_PATH_DESTINATION_TYPE)) == v::_DESTINATION_PROP as i16
                            && m.i32(at(a::CONTROL_PATH_DESTINATION_PROP_INDEX)) == m.i32(at(a::TARGET_TARGET_PROP_INDEX))
                        {
                            desires_charge = false;
                        } else if prop_distance < stop_range {
                            desires_charge = false;
                        } else if prop_distance < stop_range * 2.0
                            && dot(m.vector(at(a::INPUT_FACING_VECTOR)), m.vector(prop + p::ACTOR_TO_PROP as u32)) < 0.5
                        {
                            desires_charge = false;
                        }
                    }
                }
            }
        }

        if desires_charge && (restart_charge || action() != ACTION_CHARGE) {
            if call_simple(context, host, call::CHARGE, v::_CHARGE_GOAL_CLOSE_RANGE as i32) != 0 {
                action_changed = true;
            } else {
                desires_charge = false;
            }
        }

        if !desires_charge && !action_changed && action() != ACTION_FIGHT {
            call_simple(context, host, call::FIGHT, 0);
            action_changed = true;
        }
    }

    action_changed
}
