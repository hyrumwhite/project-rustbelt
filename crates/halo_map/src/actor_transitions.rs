//! actions.c's transition handlers, by which the actor types
//! ([`crate::actor_types`]) choose an actor's action: its initial action and
//! pending command list, what denies a transition, the panics and
//! berserking its stimuli set off, surprise, the panic transition, and
//! whether a guard or a conversation can stop.
//!
//! What they ask of the rest of the game goes through [`AiHost::call`]
//! (actions.c's `actions_port_handler_call`, shared with the C).

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
}

/// The actions (actions.h's `_actor_action_*`).
const ACTION_NONE: i16 = 0;
const ACTION_FLEE: i16 = 4;
const ACTION_OBEY: i16 = 11;

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
