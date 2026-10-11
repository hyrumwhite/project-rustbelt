//! An actor's input (actors.c's `actor_input_update`, every tick of an
//! actor's update): where its unit is and how it moves, whether it is under
//! water, flying, in a vehicle (driving, gunning or riding, and the
//! encounter the vehicle carries it into), burning to death, carrying a
//! melee attacker or a projectile about to explode, in midair, where it
//! stands for pathfinding, which way it faces, aims and looks, and its
//! unit's vitality and recent damage.
//!
//! The actor's datum and its unit's, vehicle's and children's objects are
//! the game's memory, read and written in place ([`crate::ai_actor`]); a
//! swarm's refresh, the position's sampling, the water, a vehicle's
//! definition, a gunner's weapon, the encounters, a team's enemies and a
//! unit's facing are the game's, through [`AiHost::call`] (actors.c's
//! `actor_input_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, AiStepContext, NONE, object_get};
use crate::features::{cross3, normalize3};
use crate::layout::{
    actor_datum as a, actor_definition, actor_input_data, ai_values as v, biped_datum as b, projectile_datum,
    unit_datum as u,
};

/// The input update's calls (halo_sim.h's `_actor_input_call_*`).
pub mod call {
    /// The actor's swarm's components refreshed and its center found.
    pub const SWARM_REFRESH: i32 = 1;
    /// `actor_input_sample_position(actor, a, &actor->input.position)`.
    pub const SAMPLE_POSITION: i32 = 2;
    /// Whether the unit `a`'s center of mass at the input's body location is
    /// under water, the result.
    pub const UNDERWATER: i32 = 3;
    /// The vehicle `a`'s definition's flags, the result.
    pub const VEHICLE_FLAGS: i32 = 4;
    /// Whether the actor's firing variant has a bombardment range, the result.
    pub const GUNNER_BOMBARDMENT: i32 = 5;
    /// Whether the actor stays in its squad though the vehicle's is `a`: its
    /// encounter follows a target and both squads migrate automatically.
    pub const KEEP_SQUAD: i32 = 6;
    /// `encounter_get(a)->is_prevehicle_encounter = TRUE`.
    pub const MARK_PREVEHICLE_ENCOUNTER: i32 = 7;
    /// `actor_change_encounter(actor, a, b)`.
    pub const CHANGE_ENCOUNTER: i32 = 8;
    /// The encounter's firing positions against the actor's flying (a debug
    /// count, and its error).
    pub const FIRING_POSITION_TYPE: i32 = 9;
    /// `game_team_is_enemy(a, b)`, the result.
    pub const TEAM_IS_ENEMY: i32 = 10;
    /// `unit_get_facing_vector(a)`, in `output`.
    pub const FACING: i32 = 11;
}

/// The local flags (actors.c's).
const VEHICLE_AI_WEAPON_CANNOT_ROTATE_BIT: u32 = 8;
const VEHICLE_AI_DRIVER_ENABLE_BIT: u32 = 11;
const VEHICLE_AI_DRIVER_FLYING_BIT: u32 = 12;
const VEHICLE_AI_DRIVER_NONDIRECTIONAL_BIT: u32 = 13;
const VEHICLE_AI_DRIVER_HOVERING_BIT: u32 = 14;
const PROJECTILE_WILL_SUPER_EXPLODE_BIT: u32 = 7;

const EPSILON: f32 = 0.0001;

fn call_full(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32, b: i32) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, b, ..AiCall::default() };
    host.call(&mut call);
    call
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> i32 {
    call_full(context, host, kind, a, 0).result
}

fn set_vector(m: &dyn AiMemory, address: u32, vector: [f32; 3]) {
    for (i, value) in vector.into_iter().enumerate() {
        m.set_f32(address + 4 * i as u32, value);
    }
}

fn copy_vector(m: &dyn AiMemory, to: u32, from: u32) {
    for word in 0..3 {
        m.w32(to + word * 4, m.r32(from + word * 4));
    }
}

fn test_flag(flags: u32, bit: u32) -> bool {
    flags & (1 << bit) != 0
}

/// `actor_input_update`.
pub fn actor_input_update(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    let vehicle_index_address = actor + a::INPUT_VEHICLE_INDEX as u32;

    if m.bool(actor + a::META_SWARM as u32) {
        call_simple(context, host, call::SWARM_REFRESH, 0);
        for offset in 0..actor_input_data::SIZEOF as u32 {
            m.w8(actor + a::INPUT_POSITION as u32 + offset, 0);
        }
        m.w32(vehicle_index_address, NONE as u32);
        m.w32(actor + a::INPUT_PATHFINDING_SURFACE_INDEX as u32, NONE as u32);
        let swarm_unit_index = m.i32(actor + a::META_SWARM_UNIT_INDEX as u32);
        if swarm_unit_index != NONE {
            call_simple(context, host, call::SAMPLE_POSITION, swarm_unit_index);
        }
        return;
    }

    let unit_index = m.i32(actor + a::META_UNIT_INDEX as u32);
    let unit = object_get(m, context, unit_index).unwrap_or(0);
    let vehicle_index = m.i32(unit + u::OBJECT_PARENT_OBJECT_INDEX as u32);
    let parent_object = if vehicle_index == NONE { None } else { object_get(m, context, vehicle_index) };
    let driver_type_address = actor + a::INPUT_VEHICLE_DRIVER_TYPE as u32;
    let gunner_address = actor + a::INPUT_VEHICLE_GUNNER as u32;
    let flying_address = actor + a::STATE_FLYING as u32;

    call_simple(context, host, call::SAMPLE_POSITION, unit_index);
    let underwater = call_simple(context, host, call::UNDERWATER, unit_index);
    m.w8(actor + a::INPUT_UNDERWATER as u32, underwater as u8);
    let definition_flags = m.r32(context.definition + actor_definition::FLAGS as u32);
    m.set_bool(flying_address, test_flag(definition_flags, v::_ACTOR_DEFINITION_FLYING_BIT as u32));

    match parent_object {
        Some(vehicle) if m.i16(vehicle + u::OBJECT_TYPE as u32) == v::_OBJECT_TYPE_VEHICLE as i16 => {
            let vehicle_flags = call_simple(context, host, call::VEHICLE_FLAGS, vehicle_index) as u32;

            m.w32(vehicle_index_address, vehicle_index as u32);
            m.set_bool(gunner_address, false);
            m.set_bool(actor + a::INPUT_VEHICLE_GUNNER_BOMBARDMENT as u32, false);
            m.set_i16(driver_type_address, v::_ACTOR_VEHICLE_DRIVER_NONE as i16);

            if m.i32(vehicle + u::UNIT_DRIVER_OBJECT_INDEX as u32) == unit_index {
                m.set_i16(driver_type_address, v::_ACTOR_VEHICLE_DRIVER_UNKNOWN as i16);
                if test_flag(vehicle_flags, VEHICLE_AI_DRIVER_ENABLE_BIT) {
                    if test_flag(vehicle_flags, VEHICLE_AI_DRIVER_FLYING_BIT) {
                        m.set_i16(driver_type_address, v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16);
                        m.set_bool(flying_address, true);
                    } else if test_flag(vehicle_flags, VEHICLE_AI_DRIVER_NONDIRECTIONAL_BIT) {
                        let driver_type = if test_flag(vehicle_flags, VEHICLE_AI_DRIVER_HOVERING_BIT) {
                            v::_ACTOR_VEHICLE_DRIVER_HOVERING_GROUND
                        } else {
                            v::_ACTOR_VEHICLE_DRIVER_NONDIRECTIONAL_GROUND
                        };
                        m.set_i16(driver_type_address, driver_type as i16);
                    }
                }
            }

            if m.i32(vehicle + u::UNIT_GUNNER_OBJECT_INDEX as u32) == unit_index {
                m.set_bool(gunner_address, true);
                let bombardment = call_simple(context, host, call::GUNNER_BOMBARDMENT, 0) != 0;
                m.set_bool(actor + a::INPUT_VEHICLE_GUNNER_BOMBARDMENT as u32, bombardment);
            }

            m.set_bool(
                actor + a::INPUT_VEHICLE_PASSENGER as u32,
                m.i16(driver_type_address) <= v::_ACTOR_VEHICLE_DRIVER_UNKNOWN as i16,
            );

            let fake_encounter_index = m.i16(vehicle + u::UNIT_FAKE_ENCOUNTER_INDEX as u32);
            if fake_encounter_index as i32 != NONE {
                let fake_squad_index = m.i16(vehicle + u::UNIT_FAKE_SQUAD_INDEX as u32);
                let encounter_index = m.i32(actor + a::META_ENCOUNTER_INDEX as u32);
                let migrate = if encounter_index & 0xFFFF != fake_encounter_index as i32 {
                    true
                } else if fake_squad_index as i32 != NONE
                    && m.i16(actor + a::META_SQUAD_INDEX as u32) != fake_squad_index
                {
                    call_simple(context, host, call::KEEP_SQUAD, fake_squad_index as i32) == 0
                } else {
                    false
                };

                if migrate {
                    let stored = actor + a::META_STORED_PREVEHICLE_ENCOUNTER as u32;
                    if !m.bool(stored) {
                        m.w32(actor + a::META_PREVEHICLE_ENCOUNTER_INDEX as u32, encounter_index as u32);
                        m.set_i16(
                            actor + a::META_PREVEHICLE_SQUAD_INDEX as u32,
                            m.i16(actor + a::META_SQUAD_INDEX as u32),
                        );
                        m.set_bool(stored, true);
                        if encounter_index != NONE {
                            call_simple(context, host, call::MARK_PREVEHICLE_ENCOUNTER, encounter_index);
                        }
                    }
                    call_full(
                        context,
                        host,
                        call::CHANGE_ENCOUNTER,
                        fake_encounter_index as i32,
                        fake_squad_index as i32,
                    );
                }
            }
        }
        _ => {
            m.w32(vehicle_index_address, NONE as u32);
            m.set_i16(driver_type_address, v::_ACTOR_VEHICLE_DRIVER_NONE as i16);
            m.set_bool(actor + a::INPUT_VEHICLE_PASSENGER as u32, false);
            m.set_bool(gunner_address, false);

            let stored = actor + a::META_STORED_PREVEHICLE_ENCOUNTER as u32;
            if m.bool(stored) {
                call_full(
                    context,
                    host,
                    call::CHANGE_ENCOUNTER,
                    m.i32(actor + a::META_PREVEHICLE_ENCOUNTER_INDEX as u32),
                    m.i16(actor + a::META_PREVEHICLE_SQUAD_INDEX as u32) as i32,
                );
                m.set_bool(stored, false);
            }
        }
    }

    if m.i32(actor + a::META_ENCOUNTER_INDEX as u32) != NONE {
        call_simple(context, host, call::FIRING_POSITION_TYPE, 0);
    }

    m.set_bool(actor + a::INPUT_BURNING_TO_DEATH as u32, m.r8(unit + u::UNIT_FLAMING_DEATH_DELAY as u32) > 0);
    let melee_attacker_attached = actor + a::INPUT_MELEE_ATTACKER_ATTACHED as u32;
    let delayed_projectile = actor + a::INPUT_DELAYED_ATTACHED_PROJECTILE_INDEX as u32;
    m.set_bool(melee_attacker_attached, false);
    m.w32(delayed_projectile, NONE as u32);

    let mut threat_index = m.i32(unit + u::OBJECT_FIRST_CHILD_OBJECT_INDEX as u32);
    while threat_index != NONE {
        let Some(threat) = object_get(m, context, threat_index) else { break };
        let threat_type = m.i16(threat + u::OBJECT_TYPE as u32);
        if threat_type == v::_OBJECT_TYPE_BIPED as i16
            && call_full(
                context,
                host,
                call::TEAM_IS_ENEMY,
                m.i16(actor + a::META_TEAM_INDEX as u32) as i32,
                m.i16(threat + u::OBJECT_OWNER_TEAM_INDEX as u32) as i32,
            )
            .result
                != 0
        {
            m.set_bool(melee_attacker_attached, true);
        } else if threat_type == v::_OBJECT_TYPE_PROJECTILE as i16 {
            let flags = m.r32(threat + projectile_datum::PROJECTILE_FLAGS as u32);
            if test_flag(flags, PROJECTILE_WILL_SUPER_EXPLODE_BIT)
                || (m.i16(actor + a::DANGER_ZONE_DANGER_TYPE as u32) == v::_ACTOR_DANGER_ZONE_PROJECTILE as i16
                    && threat_index == m.i32(actor + a::DANGER_ZONE_OBJECT_INDEX as u32))
            {
                m.w32(delayed_projectile, threat_index as u32);
            }
        }
        threat_index = m.i32(threat + u::OBJECT_NEXT_OBJECT_INDEX as u32);
    }

    m.set_bool(actor + a::INPUT_IN_MIDAIR as u32, false);
    m.w32(actor + a::INPUT_PATHFINDING_SURFACE_INDEX as u32, NONE as u32);
    if m.i16(unit + u::OBJECT_TYPE as u32) == v::_OBJECT_TYPE_BIPED as i16 && m.i32(vehicle_index_address) == NONE {
        if m.i8(unit + b::BIPED_AIRBORNE_TICKS as u32) >= 6 {
            m.set_bool(actor + a::INPUT_IN_MIDAIR as u32, true);
        }
        m.w32(
            actor + a::INPUT_PATHFINDING_SURFACE_INDEX as u32,
            m.r32(unit + b::BIPED_PATHFINDING_SURFACE_INDEX as u32),
        );
        copy_vector(m, actor + a::INPUT_PATHFINDING_POINT as u32, unit + b::BIPED_PATHFINDING_POINT as u32);
    }

    let facing_address = actor + a::INPUT_FACING_VECTOR as u32;
    let facing_unit = if m.i16(driver_type_address) > v::_ACTOR_VEHICLE_DRIVER_NONE as i16 {
        m.i32(vehicle_index_address)
    } else {
        unit_index
    };
    set_vector(m, facing_address, call_full(context, host, call::FACING, facing_unit, 0).vector());
    if !m.bool(flying_address) {
        let facing = m.vector(facing_address);
        let mut planar = [facing[0], facing[1]];
        let mut magnitude = (planar[0] * planar[0] + planar[1] * planar[1]).sqrt();
        if !(EPSILON > magnitude.abs()) {
            let inverse = 1.0 / magnitude;
            planar = [planar[0] * inverse, planar[1] * inverse];
        } else {
            magnitude = 0.0;
        }
        if magnitude > 0.0 {
            set_vector(m, facing_address, [planar[0], planar[1], 0.0]);
        } else {
            set_vector(m, facing_address, [1.0, 0.0, 0.0]);
        }
    }

    let aiming_address = actor + a::INPUT_AIMING_VECTOR as u32;
    if m.bool(gunner_address) {
        let gunner_vehicle_index = m.i32(vehicle_index_address);
        let gunner_vehicle = object_get(m, context, gunner_vehicle_index).unwrap_or(0);
        let flags = call_simple(context, host, call::VEHICLE_FLAGS, gunner_vehicle_index) as u32;
        if test_flag(flags, VEHICLE_AI_WEAPON_CANNOT_ROTATE_BIT) {
            set_vector(m, aiming_address, call_full(context, host, call::FACING, unit_index, 0).vector());
        } else {
            copy_vector(m, aiming_address, gunner_vehicle + u::UNIT_AIMING_VECTOR as u32);
        }
    } else {
        copy_vector(m, aiming_address, unit + u::UNIT_AIMING_VECTOR as u32);
    }

    let looking_address = actor + a::INPUT_LOOKING_VECTOR as u32;
    copy_vector(m, looking_address, unit + u::UNIT_LOOKING_VECTOR as u32);
    let looking = m.vector(looking_address);
    let mut left = cross3([0.0, 0.0, 1.0], looking);
    normalize3(&mut left);
    set_vector(m, actor + a::INPUT_LOOKING_LEFT_VECTOR as u32, left);
    set_vector(m, actor + a::INPUT_LOOKING_UP_VECTOR as u32, cross3(looking, left));

    for (to, from) in [
        (a::INPUT_BODY_VITALITY, u::OBJECT_BODY_VITALITY),
        (a::INPUT_SHIELD_VITALITY, u::OBJECT_SHIELD_VITALITY),
        (a::INPUT_RECENT_BODY_DAMAGE, u::OBJECT_RECENT_BODY_DAMAGE),
        (a::INPUT_RECENT_SHIELD_DAMAGE, u::OBJECT_RECENT_SHIELD_DAMAGE),
    ] {
        m.w32(actor + to as u32, m.r32(unit + from as u32));
    }
}
