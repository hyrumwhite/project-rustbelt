//! Bipeds' movement control (`biped_update_moving`) in Rust, on the running
//! game: what the physics wants (from the animation's root motion, the
//! throttle and player speeds, or flying), the physics itself (live, as
//! [`crate::world`] runs it), and what follows it (where it stands,
//! landing, jumping, bumps). What it asks of the game is called back at
//! the moments the C asks it ([`MovingCallbacks`]): aborting the animation,
//! the move, the AI's jump aim and the jump's footsteps, a melee impact,
//! the AI's notice of a bump, and falling damage.
//!
//! The biped update up to its animation (`biped_update`: facing, turning,
//! the movement as above, and the dead, airborne and landing updates)
//! runs in Rust too ([`halo_sim_biped_update`]); a limp body's relaxing,
//! footsteps and the game's random seed (which a tumble draws from) are the
//! game's ([`UpdateCallbacks`]).
//!
//! In check mode the game's own update runs first; Rust then replays it
//! from the state it began with, on the live world, with what the AI's jump
//! aim answered the game, as the probe replays a recorded one.

use std::sync::Arc;

use halo_map::biped::{BipedObjects, BipedPhysics, StandTest};
use halo_map::biped_moving::{
    BipedDefinition, MovingHost, MovingInputs, MovingState, PhysicsWorld, PlayerInformation, biped_flags, biped_update_moving,
};
use halo_map::biped_update::{UpdateHost, UpdateInputs, UpdateState, biped_update};

use crate::TAGS;
use crate::world::GameObjects;

/// The game's side of a biped's movement (halo_sim.c, which writes the
/// state to the biped first and reads it back after).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MovingCallbacks {
    pub abort_animation: unsafe extern "C" fn(biped: u32, state: *mut MovingState),
    pub translate: unsafe extern "C" fn(biped: u32, state: *mut MovingState, position: *const [f32; 3]),
    /// Whether its actor jumps; `velocity` the jump it would make, and the
    /// one it makes.
    pub actor_aim_jump: unsafe extern "C" fn(biped: u32, state: *mut MovingState, magnitude: f32, velocity: *mut [f32; 3]) -> i32,
    pub jumped: unsafe extern "C" fn(biped: u32, state: *mut MovingState),
    pub melee_impact: unsafe extern "C" fn(biped: u32, state: *mut MovingState, position: *const [f32; 3], new_position: *const [f32; 3]),
    pub bump_notice: unsafe extern "C" fn(biped: u32, state: *mut MovingState, object: i32, velocity: *const [f32; 3]),
    pub falling_damage: unsafe extern "C" fn(biped: u32, state: *mut MovingState, landing_velocity: f32),
    /// Its physics begins (with the stand test, if it applies) and ends,
    /// for the game's recording of it.
    pub physics_begin: unsafe extern "C" fn(biped: u32, physics: *const BipedPhysics, stand_test: i32, height: f32, radius: f32),
    pub physics_end: unsafe extern "C" fn(biped: u32, physics: *const BipedPhysics),
}

/// What a biped's movement reads, and where it leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MovingCall {
    pub biped_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub breakable_surfaces: *const u8,
    /// The state it begins with, and leaves.
    pub state: MovingState,
    pub dead: u32,
    pub ignore_translation: u32,
    pub suspended: u32,
    pub control_flags: u32,
    pub unit_animation_state: i8,
    pub base_seat_index: i8,
    pub aiming_speed: i8,
    pub actor: i8,
    pub player_index: i32,
    pub throttle: [f32; 3],
    pub body_stun: f32,
    pub recent_body_damage: f32,
    pub desired_facing_vector: [f32; 3],
    pub aiming_vector: [f32; 3],
    pub bounding_center: [f32; 3],
    /// Its animation (graph NONE for none), whose frame's root motion it
    /// moves by.
    pub animation_graph_index: i32,
    pub animation_index: i16,
    pub animation_frame_index: i16,
    pub difficulty_infection_forms: f32,
    pub speed_multiplier: f32,
    pub cinematic: u32,
    pub double_speed: u32,
    pub coop_player_collisions: u32,
    pub actor_leaping: u32,
    pub recorded_animation: u32,
    /// In check mode: what the AI's jump aim answered the game's update.
    pub actor_jump_asked: u32,
    pub actor_jumped: u32,
    pub actor_jump_velocity: [f32; 3],
    /// Written: whether it aborted the unit's animation.
    pub aborted_animation: u32,
}

impl crate::Tags {
    fn biped_definition(&mut self, definition: u32) -> Option<Arc<BipedDefinition>> {
        let cache = &self.cache;
        self.biped_definitions.entry(definition).or_insert_with(|| BipedDefinition::load(cache, definition).ok().map(Arc::new)).clone()
    }

    fn player_information(&mut self) -> Option<PlayerInformation> {
        let cache = &self.cache;
        *self.player_information.get_or_insert_with(|| PlayerInformation::load(cache).ok())
    }
}

/// The running game, answering a biped's movement.
struct LiveHost {
    callbacks: MovingCallbacks,
    biped_index: u32,
}

impl MovingHost for LiveHost {
    fn abort_animation(&self, state: &mut MovingState) {
        // SAFETY: the game's callback, with the update's state
        unsafe { (self.callbacks.abort_animation)(self.biped_index, state) };
    }

    fn translate(&self, state: &mut MovingState, position: [f32; 3]) {
        // SAFETY: as above
        unsafe { (self.callbacks.translate)(self.biped_index, state, &position) };
    }

    fn actor_aim_jump(&self, state: &mut MovingState, magnitude: f32, velocity: [f32; 3]) -> (bool, [f32; 3]) {
        let mut velocity = velocity;
        // SAFETY: as above
        let jumped = unsafe { (self.callbacks.actor_aim_jump)(self.biped_index, state, magnitude, &mut velocity) } != 0;
        (jumped, velocity)
    }

    fn jumped(&self, state: &mut MovingState) {
        // SAFETY: as above
        unsafe { (self.callbacks.jumped)(self.biped_index, state) };
    }

    fn melee_impact(&self, state: &mut MovingState, position: [f32; 3], new_position: [f32; 3]) {
        // SAFETY: as above
        unsafe { (self.callbacks.melee_impact)(self.biped_index, state, &position, &new_position) };
    }

    fn bump_notice(&self, state: &mut MovingState, object_index: i32, velocity: [f32; 3]) {
        // SAFETY: as above
        unsafe { (self.callbacks.bump_notice)(self.biped_index, state, object_index, &velocity) };
    }

    fn falling_damage(&self, state: &mut MovingState, landing_velocity: f32) {
        // SAFETY: as above
        unsafe { (self.callbacks.falling_damage)(self.biped_index, state, landing_velocity) };
    }

    fn physics_begin(&self, physics: &BipedPhysics, stand_test: Option<StandTest>) {
        let (applies, height, radius) =
            stand_test.map_or((0, 0.0, 0.0), |s| (1, s.collision_height_standing, s.collision_radius));
        // SAFETY: as above
        unsafe { (self.callbacks.physics_begin)(self.biped_index, physics, applies, height, radius) };
    }

    fn physics_end(&self, physics: &BipedPhysics) {
        // SAFETY: as above
        unsafe { (self.callbacks.physics_end)(self.biped_index, physics) };
    }
}

/// What the movement reads and its stand test, from the call.
fn moving_inputs(call: &MovingCall, check: bool) -> Option<(Arc<BipedDefinition>, PlayerInformation, MovingInputs, Option<StandTest>)> {
    let (definition, player_information, frame_info) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let tags = tags.as_mut()?;
        let definition = tags.biped_definition(call.definition_index)?;
        let player_information = tags.player_information()?;
        let frame_info = (call.animation_graph_index != -1 && call.animation_index != -1)
            .then(|| {
                halo_map::animation::frame_info(&tags.cache, call.animation_graph_index as u32, call.animation_index, call.animation_frame_index)
                    .ok()
                    .flatten()
            })
            .flatten();
        (definition, player_information, frame_info)
    };
    let inputs = MovingInputs {
        biped_index: call.biped_index,
        dead: call.dead != 0,
        ignore_translation: call.ignore_translation != 0,
        suspended: call.suspended != 0,
        control_flags: call.control_flags,
        unit_animation_state: call.unit_animation_state,
        base_seat_index: call.base_seat_index,
        aiming_speed: call.aiming_speed,
        actor: call.actor != 0,
        player_index: call.player_index,
        throttle: call.throttle,
        body_stun: call.body_stun,
        recent_body_damage: call.recent_body_damage,
        desired_facing_vector: call.desired_facing_vector,
        aiming_vector: call.aiming_vector,
        bounding_center: call.bounding_center,
        frame_info,
        difficulty_infection_forms: call.difficulty_infection_forms,
        speed_multiplier: call.speed_multiplier,
        cinematic: call.cinematic != 0,
        double_speed: call.double_speed != 0,
        coop_player_collisions: call.coop_player_collisions != 0,
        actor_leaping: call.actor_leaping != 0,
        recorded_animation: call.recorded_animation != 0,
        actor_jump: (check && call.actor_jump_asked != 0).then_some((call.actor_jumped != 0, call.actor_jump_velocity)),
    };
    // (the crouching player's stand test, for a biped whose pill stands on
    // its origin)
    let stand_test = (call.player_index != -1
        && definition.flags & (biped_flags::PILL_CENTERED_AT_ORIGIN | biped_flags::SPHERICAL) == 0)
        .then_some(StandTest {
            collision_height_standing: definition.collision_height_standing,
            collision_radius: definition.collision_radius,
        });
    Some((definition, player_information, inputs, stand_test))
}

fn run(call: &mut MovingCall, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let Some(structure) = crate::structure(call.structure_bsp_index, call.breakable_surfaces) else { return -1 };
    let Some((definition, player_information, inputs, stand_test)) = moving_inputs(call, check) else { return -1 };
    let source = GameObjects { host };
    let live = LiveHost { callbacks: host.moving, biped_index: call.biped_index };
    let world = PhysicsWorld {
        bsp: &structure.collision,
        clusters: &structure.clusters,
        objects: BipedObjects::Live { clusters: &structure.clusters, source: &source },
        host: if check { None } else { Some(&live) },
    };
    let mut state = call.state;
    let outcome = biped_update_moving(&world, &definition, &player_information, &inputs, &mut state, stand_test);
    call.state = state;
    call.aborted_animation = outcome.aborted_animation as u32;
    0
}

/// A biped's movement on the running game, in place of the game's (what it
/// asks of the game called back). Returns 0 (the state written), or -1 if
/// it can't run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its pointers the game's as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_biped_moving_update(call: *mut MovingCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of a biped's movement the game ran: Rust's replay of it from
/// the state it began with. Returns 0 (the state written), or -1.
///
/// # Safety
/// As [`halo_sim_biped_moving_update`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_biped_moving_check(call: *mut MovingCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}

/// The game's side of a biped's update (halo_sim.c, which writes the state
/// to the biped first and reads it back after).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UpdateCallbacks {
    pub moving_begins: unsafe extern "C" fn(biped: u32, state: *mut UpdateState),
    pub moving_ends: unsafe extern "C" fn(biped: u32, state: *mut UpdateState),
    pub limp_body: unsafe extern "C" fn(biped: u32, state: *mut MovingState),
    pub random_seed: unsafe extern "C" fn() -> u32,
    pub set_random_seed: unsafe extern "C" fn(seed: u32),
    pub landing_footsteps: unsafe extern "C" fn(biped: u32, state: *mut MovingState),
    pub slipping_footsteps: unsafe extern "C" fn(biped: u32, state: *mut MovingState),
}

/// What a biped's update reads (up to its animation), and where it leaves
/// it: the movement's inputs (`moving`, whose state is unused), the
/// update's state, and what it reads of the biped besides.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UpdateCall {
    pub moving: MovingCall,
    pub state: UpdateState,
    pub dead: u32,
    pub control_flags: u32,
    pub aim_without_turning: u32,
    pub unit_animation_state: i8,
    pub base_seat_index: i8,
    pub enhanced_animation: i8,
    pub limp_body: i8,
    pub player_index: i32,
    /// In check mode: the game's random seed as its airborne update began.
    pub airborne_seed_taken: u32,
    pub airborne_seed: u32,
}

/// The running game, as a biped's update calls it.
struct LiveUpdateHost {
    callbacks: UpdateCallbacks,
    biped_index: u32,
}

impl UpdateHost for LiveUpdateHost {
    fn moving_begins(&self, state: &mut UpdateState) {
        // SAFETY: the game's callback, with the update's state
        unsafe { (self.callbacks.moving_begins)(self.biped_index, state) };
    }

    fn moving_ends(&self, state: &mut UpdateState) {
        // SAFETY: as above
        unsafe { (self.callbacks.moving_ends)(self.biped_index, state) };
    }

    fn limp_body(&self, state: &mut MovingState) {
        // SAFETY: as above
        unsafe { (self.callbacks.limp_body)(self.biped_index, state) };
    }

    fn random_seed(&self) -> u32 {
        // SAFETY: as above
        unsafe { (self.callbacks.random_seed)() }
    }

    fn set_random_seed(&self, seed: u32) {
        // SAFETY: as above
        unsafe { (self.callbacks.set_random_seed)(seed) };
    }

    fn landing_footsteps(&self, state: &mut MovingState) {
        // SAFETY: as above
        unsafe { (self.callbacks.landing_footsteps)(self.biped_index, state) };
    }

    fn slipping_footsteps(&self, state: &mut MovingState) {
        // SAFETY: as above
        unsafe { (self.callbacks.slipping_footsteps)(self.biped_index, state) };
    }
}

fn run_update(call: &mut UpdateCall, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let Some(structure) = crate::structure(call.moving.structure_bsp_index, call.moving.breakable_surfaces) else { return -1 };
    let Some((definition, player_information, moving, stand_test)) = moving_inputs(&call.moving, check) else { return -1 };
    let inputs = UpdateInputs {
        dead: call.dead != 0,
        control_flags: call.control_flags,
        aim_without_turning: call.aim_without_turning != 0,
        unit_animation_state: call.unit_animation_state,
        base_seat_index: call.base_seat_index,
        enhanced_animation: call.enhanced_animation != 0,
        limp_body: call.limp_body != 0,
        player_index: call.player_index,
        airborne_seed: (check && call.airborne_seed_taken != 0).then_some(call.airborne_seed),
    };
    let source = GameObjects { host };
    let live = LiveHost { callbacks: host.moving, biped_index: call.moving.biped_index };
    let live_update = LiveUpdateHost { callbacks: host.biped_update, biped_index: call.moving.biped_index };
    let world = PhysicsWorld {
        bsp: &structure.collision,
        clusters: &structure.clusters,
        objects: BipedObjects::Live { clusters: &structure.clusters, source: &source },
        host: if check { None } else { Some(&live) },
    };
    let mut state = call.state;
    let outcome = biped_update(
        &world,
        &definition,
        &player_information,
        &inputs,
        &moving,
        stand_test,
        &[],
        &mut state,
        if check { None } else { Some(&live_update) },
    );
    call.state = state;
    call.moving.aborted_animation = outcome.aborted_animation as u32;
    0
}

/// A biped's update on the running game, up to its animation, in place of
/// the game's. Returns 0 (the state written), or -1 if it can't run (the
/// game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its pointers the game's as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_biped_update(call: *mut UpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_update(unsafe { &mut *call }, false)
}

/// The check of a biped's update the game ran: Rust's replay of it from the
/// state it began with. Returns 0 (the state written), or -1.
///
/// # Safety
/// As [`halo_sim_biped_update`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_biped_update_check(call: *mut UpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_update(unsafe { &mut *call }, true)
}
