//! The game's tick (`game.c`'s `game_tick`): the order the simulation's
//! subsystems update in, and the questions that choose some of them (a
//! distributed network client drives its units from the host's actors and
//! runs no scripts in co-op; double speed halves the effects' step).
//!
//! Each step is the game's, or a system already handed over behind it,
//! through [`GameTickHost::call`] (game.c's `game_tick_call`, shared with the
//! C).

/// The tick's steps and questions (halo_sim.h's `_game_tick_call_*`).
pub mod call {
    /// The profile, the collision log's period and the precision.
    pub const BEGIN: i32 = 1;
    /// The data arrays' order checked and the lights recovered.
    pub const DATA_CHECK: i32 = 2;
    pub const CHEATS_ENFORCE: i32 = 3;
    pub const REMOVE_QUITTING_PLAYERS: i32 = 4;
    pub const ALLEGIANCE: i32 = 5;
    pub const UNITS: i32 = 6;
    /// Whether this is a distributed network game's client (the result).
    pub const DISTRIBUTED_CLIENT: i32 = 7;
    pub const AI: i32 = 8;
    pub const NETWORK_ACTORS: i32 = 9;
    pub const PLAYERS_BEFORE: i32 = 10;
    /// Whether players are at double speed (the result).
    pub const DOUBLE_SPEED: i32 = 11;
    /// `effects_update(f)`.
    pub const EFFECTS: i32 = 12;
    /// `antennas_update(f)`.
    pub const ANTENNAS: i32 = 13;
    /// Rumble and the first person weapons, with the global seed locked.
    pub const FIRST_PERSON: i32 = 14;
    pub const GAME_ENGINE: i32 = 15;
    pub const EDITOR: i32 = 16;
    /// Whether network co-op is active (the result).
    pub const COOP_ACTIVE: i32 = 17;
    pub const SCRIPTS: i32 = 18;
    pub const RECORDED_ANIMATIONS: i32 = 19;
    pub const OBJECTS: i32 = 20;
    pub const PLAYERS_AFTER: i32 = 21;
    pub const HUD: i32 = 22;
    pub const PLAYER_EFFECTS: i32 = 23;
    /// The profile's and collision log's end.
    pub const END: i32 = 24;
}

/// `TICKS_PER_SECOND`.
const TICKS_PER_SECOND: f32 = 30.0;

/// The game's side of the tick.
pub trait GameTickHost {
    /// A step (`f` its time, if it takes one): a question's answer.
    fn call(&self, kind: i32, f: f32) -> i32;
}

/// `game_tick`.
pub fn game_tick(host: &impl GameTickHost) {
    let step = |kind| host.call(kind, 0.0);
    step(call::BEGIN);
    step(call::DATA_CHECK);
    step(call::CHEATS_ENFORCE);
    step(call::REMOVE_QUITTING_PLAYERS);
    step(call::ALLEGIANCE);
    step(call::UNITS);
    let distributed_client = step(call::DISTRIBUTED_CLIENT) != 0;
    if !distributed_client {
        step(call::AI);
    } else {
        step(call::NETWORK_ACTORS);
    }
    step(call::PLAYERS_BEFORE);
    let seconds_per_tick =
        if step(call::DOUBLE_SPEED) != 0 { 1.0 / (2.0 * TICKS_PER_SECOND) } else { 1.0 / TICKS_PER_SECOND };
    host.call(call::EFFECTS, seconds_per_tick);
    host.call(call::ANTENNAS, seconds_per_tick);
    step(call::FIRST_PERSON);
    step(call::GAME_ENGINE);
    step(call::EDITOR);
    // (`network_game_distributed_client() && network_coop_active()`: co-op
    // asked only of a client)
    let distributed_client = step(call::DISTRIBUTED_CLIENT) != 0;
    if !(distributed_client && step(call::COOP_ACTIVE) != 0) {
        step(call::SCRIPTS);
    }
    step(call::RECORDED_ANIMATIONS);
    step(call::OBJECTS);
    step(call::PLAYERS_AFTER);
    step(call::HUD);
    step(call::PLAYER_EFFECTS);
    step(call::DATA_CHECK);
    step(call::END);
}
