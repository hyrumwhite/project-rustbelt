//! An actor's path to its destination (actor_moving.c's `actor_path_refresh`
//! and what orders it: `actor_move_to_point`, `actor_move_to_move_position`,
//! `actor_move_to_firing_position`, `actor_move_to_prop`, `actor_move_halt`
//! and `actor_move_halt_at_firing_position`, with `actor_path_clear` and
//! `actor_test_destination`), for any step that moves an actor: the
//! destination update and the actions.
//!
//! The actor's datum, its props and the scenario's encounters are the game's
//! memory, worked on in place ([`crate::ai_actor`]); waking the actor, a
//! prop's pathfinding location, the destination tolerance, the flyers' test
//! and path, and the walkers' path (its input, attractor and search, Rust's
//! own through the game's `paths`) are the game's, through [`AiHost::call`]
//! (actor_moving.c's `actor_path_call`, shared with the C).

use crate::ai_actor::{AiCall, AiHost, AiMemoryExt, AiStepContext, NONE, prop_get, tag_block_count, tag_block_element};
use crate::layout::{
    actor_datum as a, actor_debug_info, actor_destination_orders as o, ai_values as v, encounter_definition,
    firing_position_definition, move_position_definition, path_destination, path_result, prop_datum as p, scenario,
    squad_definition,
};

/// The path's calls (halo_sim.h's `_actor_path_call_*`), in a range of their
/// own so that any step's call function can take them.
pub mod call {
    const BASE: i32 = 15 << 8;
    /// `actor_set_dormant(actor, FALSE)`, for an active, dormant actor.
    pub const SET_AWAKE: i32 = BASE | 1;
    /// `actor_perception_find_prop_pathfinding_location(actor, a)`.
    pub const FIND_PROP_PATHFINDING_LOCATION: i32 = BASE | 2;
    /// `actor_path_3d_available` to the actor's destination, the result.
    pub const PATH_3D_AVAILABLE: i32 = BASE | 3;
    /// `actor_destination_tolerance(actor)`, in `output[0]`.
    pub const DESTINATION_TOLERANCE: i32 = BASE | 4;
    /// `path_3d_build_path` from the actor to its destination into its path,
    /// the result.
    pub const BUILD_3D: i32 = BASE | 5;
    /// The walkers' path to the actor's destination into its path (its
    /// input, the target it ignores, the danger zone's attractor, the search
    /// and the build), the result.
    pub const BUILD: i32 = BASE | 6;
    // 7 is the game's alone (a cached path state).
    /// `actor_find_pathfinding_location(actor)`.
    pub const FIND_PATHFINDING_LOCATION: i32 = BASE | 8;
    /// `seed_random(global seed)`, the result.
    pub const SEED_RANDOM: i32 = BASE | 9;
}

fn call_simple(context: &AiStepContext, host: &impl AiHost, kind: i32, a: i32) -> AiCall {
    let mut call = AiCall { kind, actor: context.actor_index, a, ..AiCall::default() };
    host.call(&mut call);
    call
}

/// `distance_squared3d`.
fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn set_vector(m: &dyn crate::ai_actor::AiMemory, address: u32, vector: [f32; 3]) {
    for (i, value) in vector.into_iter().enumerate() {
        m.set_f32(address + 4 * i as u32, value);
    }
}

/// `actor_path_clear`.
pub fn actor_path_clear(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    let actor = context.actor;
    m.set_bool(actor + (a::CONTROL_PATH_PATH + path_result::VALID) as u32, false);
    m.set_bool(actor + a::CONTROL_PATH_AT_DESTINATION as u32, true);
    m.set_f32(actor + a::CONTROL_PATH_DESTINATION_ORIGINAL_DISTANCE as u32, 0.0);
}

/// `actor_test_destination`: whether the actor is at its destination (and
/// marks it so).
pub fn actor_test_destination(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let actor = context.actor;
    let at_destination = actor + a::CONTROL_PATH_AT_DESTINATION as u32;
    let destination_type = m.i16(actor + (a::CONTROL_PATH_DESTINATION_ORDERS + o::DESTINATION_TYPE) as u32);
    if destination_type == v::_DESTINATION_NONE as i16 || destination_type == v::_DESTINATION_HALT as i16 {
        m.set_bool(at_destination, true);
    } else {
        let tolerance = call_simple(context, host, call::DESTINATION_TOLERANCE, 0).output[0];
        if distance_squared(
            m.vector(actor + a::INPUT_BODY_POSITION as u32),
            m.vector(actor + (a::CONTROL_PATH_DESTINATION + path_destination::POINT) as u32),
        ) < tolerance * tolerance
        {
            m.set_bool(at_destination, true);
        }
    }
    m.bool(at_destination)
}

/// `actor_path_refresh` (without a cached path state, which only the
/// game's firing position change passes): the destination from the
/// actor's destination orders, and a new path to it when it isn't there.
pub fn actor_path_refresh(context: &AiStepContext, host: &impl AiHost, new_destination: bool) -> bool {
    let m = host.memory();
    let actor = context.actor;
    let at = |offset: usize| actor + offset as u32;
    let orders = |offset: usize| at(a::CONTROL_PATH_DESTINATION_ORDERS + offset);
    let destination = |offset: usize| at(a::CONTROL_PATH_DESTINATION + offset);
    let destination_type = m.i16(orders(o::DESTINATION_TYPE));
    let none = v::_DESTINATION_NONE as i16;
    let halt = v::_DESTINATION_HALT as i16;
    let mut success = true;

    let previous_destination = (destination_type != none && destination_type != halt)
        .then(|| m.vector(destination(path_destination::POINT)));

    if m.bool(at(a::INPUT_VEHICLE_PASSENGER))
        || destination_type == none
        || destination_type == halt
        || (destination_type == v::_DESTINATION_FIRING_POSITION as i16
            && m.bool(at(a::FIRING_POSITIONS_MOVED_AWAY_FROM_FIRING_POSITION)))
    {
        actor_path_clear(context, host);
        return success;
    }

    m.set_bool(at(a::CONTROL_PATH_PATH + path_result::VALID), false);
    m.set_bool(at(a::CONTROL_PATH_AT_DESTINATION), false);
    m.set_f32(at(a::CONTROL_PATH_DESTINATION_ORIGINAL_DISTANCE), 0.0);
    m.set_bool(at(a::CONTROL_MOVEMENT_COMPLETE), false);

    let encounter_index = m.i32(at(a::META_ENCOUNTER_INDEX));
    let encounter = || {
        tag_block_element(
            m,
            context.scenario + scenario::AI_ENCOUNTERS as u32,
            encounter_index & 0xFFFF,
            encounter_definition::SIZEOF,
        )
    };
    let flying = m.bool(at(a::STATE_FLYING));
    let set_destination = |point: [f32; 3], surface_index: i32, target_radius: f32| {
        set_vector(m, destination(path_destination::POINT), point);
        m.w32(destination(path_destination::SURFACE_INDEX), surface_index as u32);
        m.set_f32(destination(path_destination::TARGET_RADIUS), target_radius);
    };
    match destination_type as usize {
        v::_DESTINATION_RAW_LOCATION => {
            set_destination(m.vector(orders(o::RAW_POINT)), m.i32(orders(o::RAW_SURFACE_INDEX)), 0.0);
        }
        v::_DESTINATION_MOVE_POSITION => {
            success = false;
            if encounter_index != NONE {
                let squad = tag_block_element(
                    m,
                    encounter() + encounter_definition::SQUADS as u32,
                    m.i16(at(a::META_SQUAD_INDEX)) as i32,
                    squad_definition::SIZEOF,
                );
                let move_positions = squad + squad_definition::MOVE_POSITIONS as u32;
                let move_position_index = m.i16(orders(o::MOVE_POSITION_INDEX)) as i32;
                if move_position_index >= 0 && move_position_index < tag_block_count(m, move_positions) {
                    let move_position =
                        tag_block_element(m, move_positions, move_position_index, move_position_definition::SIZEOF);
                    success = true;
                    set_destination(
                        m.vector(move_position + move_position_definition::POSITION as u32),
                        m.i32(move_position + move_position_definition::SURFACE_INDEX as u32),
                        0.0,
                    );
                }
            }
        }
        v::_DESTINATION_FIRING_POSITION => {
            if encounter_index == NONE {
                success = false;
            } else {
                let firing_position = tag_block_element(
                    m,
                    encounter() + encounter_definition::FIRING_POSITIONS as u32,
                    m.i16(orders(o::FIRING_POSITION_INDEX)) as i32,
                    firing_position_definition::SIZEOF,
                );
                set_destination(
                    m.vector(firing_position + firing_position_definition::POSITION as u32),
                    m.i32(firing_position + firing_position_definition::SURFACE_INDEX as u32),
                    0.0,
                );
            }
        }
        v::_DESTINATION_PROP => {
            let prop_index = m.i32(orders(o::PROP_INDEX));
            let prop = prop_get(m, context, prop_index);
            let state = m.i16(prop + p::STATE as u32);
            if state < v::_PROP_STATE_UNINSPECTED_ORPHAN as i16 || state > v::_PROP_STATE_INSPECTED_ORPHAN as i16 {
                call_simple(context, host, call::FIND_PROP_PATHFINDING_LOCATION, prop_index);
            }
            let point = if flying {
                m.vector(prop + p::CENTER_OF_MASS as u32)
            } else {
                m.vector(prop + p::PATHFINDING_POINT as u32)
            };
            set_destination(
                point,
                m.i32(prop + p::PATHFINDING_SURFACE_INDEX as u32),
                m.f32(orders(o::PROP_ACCEPT_RADIUS)),
            );
        }
        _ => success = false,
    }

    if success {
        let path_available = if flying {
            call_simple(context, host, call::PATH_3D_AVAILABLE, 0).result != 0
        } else if m.f32(destination(path_destination::TARGET_RADIUS)) == 0.0 {
            m.i32(destination(path_destination::SURFACE_INDEX)) != NONE
        } else {
            true
        };
        if !path_available {
            success = false;
        }
    }

    if success {
        let build_path = if actor_test_destination(context, host) {
            previous_destination.is_some_and(|previous| {
                distance_squared(previous, m.vector(destination(path_destination::POINT))) > 0.1f32 * 0.1f32
            })
        } else {
            true
        };

        if build_path {
            // distance3d: the C's double square root
            let distance = (distance_squared(
                m.vector(at(a::INPUT_BODY_POSITION)),
                m.vector(destination(path_destination::POINT)),
            ) as f64)
                .sqrt() as f32;
            m.w32(context.debug_info + actor_debug_info::LAST_PATH_REFRESH as u32, context.game_time as u32);

            success = call_simple(context, host, if flying { call::BUILD_3D } else { call::BUILD }, 0).result != 0;

            m.set_bool(at(a::CONTROL_PATH_REFRESHED_THIS_TICK), true);
            if new_destination {
                m.set_f32(at(a::CONTROL_PATH_DESTINATION_ORIGINAL_DISTANCE), distance);
            }

            if success
                && m.f32(at(a::CONTROL_PATH_PATH_ENDPOINT_TARGET_RADIUS)) > 0.0
                && distance < m.f32(destination(path_destination::TARGET_RADIUS))
                && distance - m.f32(at(a::CONTROL_PATH_PATH_ENDPOINT_TARGET_RADIUS)) < 0.5
            {
                actor_path_clear(context, host);
            }
        }
    }

    if !success {
        actor_path_clear(context, host);
    }

    success
}

/// `actor_set_dormant(actor, FALSE)`: through the game when it changes
/// anything.
fn set_awake(context: &AiStepContext, host: &impl AiHost) {
    let m = host.memory();
    if m.bool(context.actor + a::META_ACTIVE as u32) && m.bool(context.actor + a::META_DORMANT as u32) {
        call_simple(context, host, call::SET_AWAKE, 0);
    }
}

/// The move orders' destination becomes the path's, and a new path is
/// found; or the path is refreshed in the actor's timeslice.
fn order_or_refresh(context: &AiStepContext, host: &impl AiHost, new_orders: Option<&dyn Fn()>) -> bool {
    let m = host.memory();
    let actor = context.actor;
    match new_orders {
        Some(write_orders) => {
            write_orders();
            for offset in 0..o::SIZEOF as u32 {
                m.w8(
                    actor + a::CONTROL_PATH_DESTINATION_ORDERS as u32 + offset,
                    m.r8(actor + a::ORDERS_MOVE_DESTINATION as u32 + offset),
                );
            }
            actor_path_refresh(context, host, true)
        }
        None if m.bool(actor + a::META_TIMESLICE as u32) && !m.bool(actor + a::CONTROL_PATH_REFRESHED_THIS_TICK as u32) => {
            actor_path_refresh(context, host, false)
        }
        None => true,
    }
}

/// `actor_move_to_point`.
pub fn actor_move_to_point(
    context: &AiStepContext,
    host: &impl AiHost,
    point: [f32; 3],
    surface_index: i32,
    ignore_target_object_index: i32,
) -> bool {
    let m = host.memory();
    let actor = context.actor;
    let path_orders = |offset: usize| actor + (a::CONTROL_PATH_DESTINATION_ORDERS + offset) as u32;
    let move_orders = |offset: usize| actor + (a::ORDERS_MOVE_DESTINATION + offset) as u32;
    m.set_i16(actor + a::FIRING_POSITIONS_CURRENT_POSITION_INDEX as u32, NONE as i16);
    set_awake(context, host);
    let changed = m.i16(path_orders(o::DESTINATION_TYPE)) != v::_DESTINATION_RAW_LOCATION as i16
        || m.i32(path_orders(o::RAW_SURFACE_INDEX)) != surface_index
        || distance_squared(point, m.vector(path_orders(o::RAW_POINT))) > 0.1f32 * 0.1f32;
    let write = || {
        m.set_i16(move_orders(o::DESTINATION_TYPE), v::_DESTINATION_RAW_LOCATION as i16);
        m.set_bool(move_orders(o::KEEP_MOVING), false);
        set_vector(m, move_orders(o::RAW_POINT), point);
        m.w32(move_orders(o::RAW_SURFACE_INDEX), surface_index as u32);
        m.w32(move_orders(o::IGNORE_TARGET_OBJECT_INDEX), ignore_target_object_index as u32);
    };
    order_or_refresh(context, host, changed.then_some(&write as &dyn Fn()))
}

/// `actor_move_to_move_position`.
pub fn actor_move_to_move_position(context: &AiStepContext, host: &impl AiHost, move_position_index: i16) -> bool {
    let m = host.memory();
    let actor = context.actor;
    let path_orders = |offset: usize| actor + (a::CONTROL_PATH_DESTINATION_ORDERS + offset) as u32;
    let move_orders = |offset: usize| actor + (a::ORDERS_MOVE_DESTINATION + offset) as u32;
    m.set_i16(actor + a::FIRING_POSITIONS_CURRENT_POSITION_INDEX as u32, NONE as i16);
    set_awake(context, host);
    let changed = m.i16(path_orders(o::DESTINATION_TYPE)) != v::_DESTINATION_MOVE_POSITION as i16
        || m.i16(path_orders(o::MOVE_POSITION_INDEX)) != move_position_index;
    let write = || {
        m.set_i16(move_orders(o::DESTINATION_TYPE), v::_DESTINATION_MOVE_POSITION as i16);
        m.set_bool(move_orders(o::KEEP_MOVING), false);
        m.set_i16(move_orders(o::MOVE_POSITION_INDEX), move_position_index);
        m.w32(move_orders(o::IGNORE_TARGET_OBJECT_INDEX), NONE as u32);
    };
    order_or_refresh(context, host, changed.then_some(&write as &dyn Fn()))
}

/// `actor_move_to_firing_position` (without a cached path state).
pub fn actor_move_to_firing_position(context: &AiStepContext, host: &impl AiHost, firing_position_index: i16) -> bool {
    let m = host.memory();
    let actor = context.actor;
    let path_orders = |offset: usize| actor + (a::CONTROL_PATH_DESTINATION_ORDERS + offset) as u32;
    let move_orders = |offset: usize| actor + (a::ORDERS_MOVE_DESTINATION + offset) as u32;
    set_awake(context, host);
    let changed = m.i16(path_orders(o::DESTINATION_TYPE)) != v::_DESTINATION_FIRING_POSITION as i16
        || m.i16(path_orders(o::FIRING_POSITION_INDEX)) != firing_position_index;
    let write = || {
        m.set_i16(move_orders(o::DESTINATION_TYPE), v::_DESTINATION_FIRING_POSITION as i16);
        m.set_bool(move_orders(o::KEEP_MOVING), false);
        m.set_i16(move_orders(o::FIRING_POSITION_INDEX), firing_position_index);
        m.w32(move_orders(o::IGNORE_TARGET_OBJECT_INDEX), NONE as u32);
        m.set_bool(actor + a::FIRING_POSITIONS_MOVED_AWAY_FROM_FIRING_POSITION as u32, false);
    };
    order_or_refresh(context, host, changed.then_some(&write as &dyn Fn()))
}

/// `actor_move_to_prop`.
pub fn actor_move_to_prop(context: &AiStepContext, host: &impl AiHost, prop_index: i32, accept_radius: f32) -> bool {
    let m = host.memory();
    let actor = context.actor;
    let path_orders = |offset: usize| actor + (a::CONTROL_PATH_DESTINATION_ORDERS + offset) as u32;
    let move_orders = |offset: usize| actor + (a::ORDERS_MOVE_DESTINATION + offset) as u32;
    m.set_i16(actor + a::FIRING_POSITIONS_CURRENT_POSITION_INDEX as u32, NONE as i16);
    set_awake(context, host);
    let changed = m.i16(path_orders(o::DESTINATION_TYPE)) != v::_DESTINATION_PROP as i16
        || m.i32(path_orders(o::PROP_INDEX)) != prop_index
        || m.f32(path_orders(o::PROP_ACCEPT_RADIUS)) != accept_radius;
    let write = || {
        let prop = prop_get(m, context, prop_index);
        let vehicle_index = m.i32(prop + p::VEHICLE_INDEX as u32);
        m.set_i16(move_orders(o::DESTINATION_TYPE), v::_DESTINATION_PROP as i16);
        m.set_bool(move_orders(o::KEEP_MOVING), false);
        m.w32(move_orders(o::PROP_INDEX), prop_index as u32);
        m.set_f32(move_orders(o::PROP_ACCEPT_RADIUS), accept_radius);
        let ignore = if vehicle_index == NONE { m.i32(prop + p::UNIT_INDEX as u32) } else { vehicle_index };
        m.w32(move_orders(o::IGNORE_TARGET_OBJECT_INDEX), ignore as u32);
    };
    order_or_refresh(context, host, changed.then_some(&write as &dyn Fn()))
}

/// The halt orders become the path's.
fn order_halt(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let actor = context.actor;
    if m.i16(actor + (a::CONTROL_PATH_DESTINATION_ORDERS + o::DESTINATION_TYPE) as u32) != v::_DESTINATION_HALT as i16 {
        m.set_i16(actor + (a::ORDERS_MOVE_DESTINATION + o::DESTINATION_TYPE) as u32, v::_DESTINATION_HALT as i16);
        for offset in 0..o::SIZEOF as u32 {
            m.w8(
                actor + a::CONTROL_PATH_DESTINATION_ORDERS as u32 + offset,
                m.r8(actor + a::ORDERS_MOVE_DESTINATION as u32 + offset),
            );
        }
    }
    actor_path_refresh(context, host, true)
}

/// `actor_move_halt`.
pub fn actor_move_halt(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let actor = context.actor;
    if m.i16(actor + a::INPUT_VEHICLE_DRIVER_TYPE as u32) == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16
        && m.bool(actor + a::CONTROL_MOVING as u32)
    {
        return actor_move_to_point(
            context,
            host,
            m.vector(actor + a::INPUT_BODY_POSITION as u32),
            m.i32(actor + a::INPUT_PATHFINDING_SURFACE_INDEX as u32),
            NONE,
        );
    }
    m.set_i16(actor + a::FIRING_POSITIONS_CURRENT_POSITION_INDEX as u32, NONE as i16);
    order_halt(context, host)
}

/// `actor_move_halt_at_firing_position`.
pub fn actor_move_halt_at_firing_position(context: &AiStepContext, host: &impl AiHost) -> bool {
    let m = host.memory();
    let actor = context.actor;
    if m.i16(actor + a::INPUT_VEHICLE_DRIVER_TYPE as u32) == v::_ACTOR_VEHICLE_DRIVER_DIRECTIONAL_FLYING as i16 {
        let firing_position_index = m.i16(actor + a::FIRING_POSITIONS_CURRENT_POSITION_INDEX as u32);
        if firing_position_index == NONE as i16 {
            return actor_move_halt(context, host);
        }
        return actor_move_to_firing_position(context, host, firing_position_index);
    }
    order_halt(context, host)
}
