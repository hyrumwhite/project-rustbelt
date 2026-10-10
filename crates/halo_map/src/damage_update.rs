//! An object's damage over time (ROADMAP phase 7, objects and physics):
//! `object_damage_update`, from OpenCE's `source/objects/damage.c`, ported
//! as it is, in the same single-precision arithmetic. Each tick, for each
//! object with a collision model: an act of god's death, the shield's
//! over-charge (rising to three times, or draining above full in a
//! gametype), its recharge once its stun has run out, and the decay of the
//! recent and current body and shield damage.
//!
//! What it asks of the game goes through a [`DamageUpdateHost`] ([`call`]'s
//! kinds).

use crate::damage::ObjectDamageState;

const NONE: i32 = -1;
const TICKS_PER_SECOND: i32 = 30;

// object damage flags
const DEAD: u16 = 1 << 2;
const SHIELD_DEPLETED: u16 = 1 << 3;
const SHIELD_OVER_CHARGING: u16 = 1 << 4;
const DIE_ACT_OF_GOD: u16 = 1 << 5;
const DIE_ACT_OF_GOD_SILENT: u16 = 1 << 6;
const SHIELD_CHARGING: u16 = 1 << 12;
const DIE_ACT_OF_GOD_NO_STATISTICS: u16 = 1 << 13;

/// What the update asks of the game (`call`'s kind).
pub mod call {
    /// An act of god's death: the game's falling damage, killing instantly
    /// (silently, or with no statistics, as the flags say).
    pub const ACT_OF_GOD: i32 = 1;
    /// `hud_tick_shield` of the object's player: `f` the over-charge drained.
    pub const HUD_TICK_SHIELD: i32 = 2;
    /// `game_difficulty_get_team_value` of the enemies' recharge for the
    /// object's team: out.
    pub const DIFFICULTY_ENEMY_RECHARGE: i32 = 3;
    /// A depleted shield begins to recharge: its recharging effect, and its
    /// shield regions' permutations (`object_permutation_shield_regions`).
    pub const SHIELD_RECHARGING: i32 = 4;
}

/// What the update reads of the game besides the object.
#[derive(Debug, Clone, Copy, Default)]
pub struct DamageUpdateInputs {
    /// `objects_update_shields` (not a distributed game's client).
    pub update_shields: bool,
    pub game_engine_running: bool,
    /// The collision model's `runtime_shield_recharge_velocity`.
    pub shield_recharge_velocity: f32,
}

/// A running game, as the update asks it. The object's state is the
/// update's as it stands (the host writes it to the game first, and reads
/// back what the call changed).
pub trait DamageUpdateHost {
    /// A call: its result, and a value out.
    fn call(&self, state: &mut ObjectDamageState, kind: i32, f: f32) -> (i32, f32);
}

/// `object_damage_update`, for an object with a collision model.
pub fn object_damage_update(inputs: &DamageUpdateInputs, host: &dyn DamageUpdateHost, object: &mut ObjectDamageState) {
    let damage_flags = object.damage_flags;
    if damage_flags & DIE_ACT_OF_GOD_NO_STATISTICS != 0 || damage_flags & (DIE_ACT_OF_GOD | DIE_ACT_OF_GOD_SILENT) != 0 {
        if damage_flags & DEAD == 0 {
            host.call(object, call::ACT_OF_GOD, 0.0);
        }
        object.damage_flags &= !(DIE_ACT_OF_GOD | DIE_ACT_OF_GOD_SILENT | DIE_ACT_OF_GOD_NO_STATISTICS);
    }

    if inputs.update_shields {
        object.damage_flags &= !SHIELD_CHARGING;
    }
    let damage_flags = object.damage_flags;
    if inputs.update_shields && object.maximum_shield_vitality > 0.0 && damage_flags & DEAD == 0 {
        if damage_flags & SHIELD_OVER_CHARGING != 0 {
            let shield_vitality = object.shield_vitality + 0.033333335;
            object.shield_vitality = shield_vitality;
            if shield_vitality >= 3.0 {
                object.shield_vitality = 3.0;
                object.damage_flags &= !SHIELD_OVER_CHARGING;
            } else {
                object.damage_flags |= SHIELD_CHARGING;
            }
        } else if object.shield_vitality > 1.0 && inputs.game_engine_running {
            let overcharge = object.shield_vitality - 1.0;
            if 0.00074074074 > overcharge {
                object.shield_vitality = 1.0;
                host.call(object, call::HUD_TICK_SHIELD, overcharge);
            } else {
                object.shield_vitality -= 0.00074074074;
                host.call(object, call::HUD_TICK_SHIELD, 0.00074074074);
            }
        } else if object.shield_vitality < 1.0 {
            if object.shield_stun_ticks == 0 {
                let mut shield_recharge = inputs.shield_recharge_velocity;
                shield_recharge *= host.call(object, call::DIFFICULTY_ENEMY_RECHARGE, 0.0).1;

                if object.damage_flags & SHIELD_DEPLETED != 0 {
                    host.call(object, call::SHIELD_RECHARGING, 0.0);
                    object.damage_flags &= !SHIELD_DEPLETED;
                }

                object.damage_flags |= SHIELD_CHARGING;
                shield_recharge += object.shield_vitality;
                object.shield_vitality = shield_recharge;
                if shield_recharge > 1.0 {
                    object.damage_flags &= !SHIELD_CHARGING;
                    object.shield_vitality = 1.0;
                }
            } else {
                object.shield_stun_ticks -= 1;
            }
        }
    }

    if object.body_damage_decay_timer != NONE {
        object.body_damage_decay_timer += 1;
        let decay_timer = object.body_damage_decay_timer;
        if decay_timer >= 0 {
            object.current_body_damage -= 0.016666668;
        }
        if decay_timer >= TICKS_PER_SECOND * 2 {
            object.recent_body_damage -= 0.016666668;
        }
        object.current_body_damage = max0(object.current_body_damage);
        object.recent_body_damage = max0(object.recent_body_damage);
        if object.current_body_damage == 0.0 && object.recent_body_damage == 0.0 {
            object.body_damage_decay_timer = NONE;
        }
    }

    if object.shield_damage_decay_timer != NONE {
        object.shield_damage_decay_timer += 1;
        let decay_timer = object.shield_damage_decay_timer;
        if decay_timer >= 0 {
            object.current_shield_damage -= 0.016666668;
        }
        if decay_timer >= TICKS_PER_SECOND * 2 {
            object.recent_shield_damage -= 0.016666668;
        }
        object.current_shield_damage = max0(object.current_shield_damage);
        object.recent_shield_damage = max0(object.recent_shield_damage);
        if object.current_shield_damage == 0.0 && object.recent_shield_damage == 0.0 {
            object.shield_damage_decay_timer = NONE;
        }
    }
}

/// `MAX(0.f, value)`.
fn max0(value: f32) -> f32 {
    if 0.0 > value { 0.0 } else { value }
}
