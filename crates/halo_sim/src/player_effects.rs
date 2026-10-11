//! Player effects in Rust, on the running game: a local player's screen
//! flash, camera impulse, shake, continuous shake and damage indicators are
//! `halo_map::player_effects`'; what they take of the rest of the game (the
//! player's facing and permanent impulse, rumble, a sound, the attacker from
//! the camera) is the game's, through player_effects.c's `player_effect_call`
//! ([`PlayerEffectCallbacks`]).
//!
//! In check mode the game's own operation runs first, recording each call
//! with the state before and after; Rust replays it from the state as it
//! was, and the game compares the state and output Rust leaves.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::object_functions::FunctionTables;
use halo_map::player_effects::{
    ContinuousEffectView, DamageEffectView, PlayerEffectCall, PlayerEffectHost, PlayerEffectInputs, PlayerEffectOp,
    PlayerEffectState, RenderScreenFlash, ScreenFlash, player_effect,
};

use crate::TAGS;

/// The game's side of player effects (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PlayerEffectCallbacks {
    /// A call: the state written to the game first, read back after.
    pub call: unsafe extern "C" fn(state: *mut PlayerEffectState, call: *mut PlayerEffectCall),
}

/// A call of the game's operation, recorded in check mode.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PlayerEffectEventC {
    pub call: PlayerEffectCall,
    pub before: PlayerEffectState,
    pub after: PlayerEffectState,
}

/// halo_sim.h's `_player_effect_op_*`.
pub mod op {
    pub const START: i32 = 1;
    pub const SCREEN_FLASH: i32 = 2;
    pub const TELEFRAG: i32 = 3;
    pub const CONTINUOUS: i32 = 4;
    pub const CAMERA_MATRIX: i32 = 5;
    pub const GET_SCREEN_FLASH: i32 = 6;
}

/// A player effect operation, what it reads, and what it leaves.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PlayerEffectCallC {
    pub op: i32,
    /// A damage effect (`START`) or continuous damage effect (`CONTINUOUS`).
    pub definition_index: i32,
    pub owner_object_index: i32,
    pub direction: [f32; 3],
    pub scale: f32,
    pub total_damage: f32,
    pub intensity: f32,
    pub distance: f32,
    pub screen_flash: ScreenFlash,
    pub game_time: i32,
    pub elapsed: i16,
    pub console_active: u8,
    pub pad: u8,
    pub periodic_tables: [*const u8; 12],
    pub transition_tables: [*const u8; 6],
    pub directions: *const [f32; 3],
    pub direction_count: u32,
    pub state: PlayerEffectState,
    /// `GET_SCREEN_FLASH`'s, in (the caller's) and out.
    pub screen_flash_out: RenderScreenFlash,
    /// `CAMERA_MATRIX`'s: scale, forward, left, up, position; whether one was
    /// made.
    pub matrix: [f32; 13],
    pub has_matrix: u32,
    /// In check mode: the game's calls, and the first that differs (as the
    /// other systems').
    pub events: *const PlayerEffectEventC,
    pub event_count: u32,
    pub difference_event: i32,
    pub difference_reason: i32,
}

impl crate::Tags {
    fn damage_effect_view(&mut self, datum: u32) -> Option<Arc<DamageEffectView>> {
        if let Some(view) = self.damage_effect_views.get(&datum) {
            return view.clone();
        }
        let view = DamageEffectView::load(&self.cache, datum).ok().map(Arc::new);
        self.damage_effect_views.insert(datum, view.clone());
        view
    }

    fn continuous_effect_view(&mut self, datum: u32) -> Option<Arc<ContinuousEffectView>> {
        if let Some(view) = self.continuous_effect_views.get(&datum) {
            return view.clone();
        }
        let view = ContinuousEffectView::load(&self.cache, datum).ok().map(Arc::new);
        self.continuous_effect_views.insert(datum, view.clone());
        view
    }
}

fn state_bytes(state: &PlayerEffectState) -> &[u8] {
    // SAFETY: plain repr(C) data; its padding bytes are the pad fields
    unsafe { std::slice::from_raw_parts((state as *const PlayerEffectState).cast::<u8>(), size_of::<PlayerEffectState>()) }
}

struct LiveHost {
    callbacks: PlayerEffectCallbacks,
}

impl PlayerEffectHost for LiveHost {
    fn call(&self, state: &mut PlayerEffectState, call: &mut PlayerEffectCall) {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.call)(state, call) };
    }
}

struct CheckHost<'a> {
    events: &'a [PlayerEffectEventC],
    next: Cell<usize>,
    difference: Cell<Option<(usize, i32)>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }
}

impl PlayerEffectHost for CheckHost<'_> {
    fn call(&self, state: &mut PlayerEffectState, call: &mut PlayerEffectCall) {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return;
        };
        self.next.set(index + 1);
        let e = &event.call;
        if e.kind != call.kind {
            self.differ(index, 1);
        } else if e.a != call.a || e.b != call.b || e.f.map(f32::to_bits) != call.f.map(f32::to_bits) {
            self.differ(index, 2);
        } else if state_bytes(&event.before) != state_bytes(state) {
            self.differ(index, 3);
        }
        *state = event.after;
        call.result = e.result;
        call.out = e.out;
    }
}

fn run(call: &mut PlayerEffectCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let (damage, continuous) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        match call.op {
            op::START => match tags.damage_effect_view(call.definition_index as u32) {
                Some(view) => (Some(view), None),
                None => return -1,
            },
            op::CONTINUOUS => match tags.continuous_effect_view(call.definition_index as u32) {
                Some(view) => (None, Some(view)),
                None => return -1,
            },
            _ => (None, None),
        }
    };
    let mut functions = FunctionTables::default();
    for (slot, &pointer) in functions.periodic.iter_mut().zip(call.periodic_tables.iter()) {
        // SAFETY: the game's tables, 1024 bytes each
        *slot = (!pointer.is_null()).then(|| unsafe { &*(pointer as *const [u8; 1024]) });
    }
    for (slot, &pointer) in functions.transition.iter_mut().zip(call.transition_tables.iter()) {
        // SAFETY: as above
        *slot = (!pointer.is_null()).then(|| unsafe { &*(pointer as *const [u8; 1024]) });
    }
    // SAFETY: the game's direction table
    let directions = if call.directions.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) }
    };
    let inputs = PlayerEffectInputs {
        game_time: call.game_time,
        elapsed: call.elapsed,
        console_active: call.console_active != 0,
        functions: &functions,
        directions,
    };
    let (no_damage, no_continuous) = (DamageEffectView::default(), ContinuousEffectView::default());
    let operation = match call.op {
        op::START => PlayerEffectOp::Start {
            definition_index: call.definition_index,
            definition: damage.as_deref().unwrap_or(&no_damage),
            owner_object_index: call.owner_object_index,
            direction: call.direction,
            scale: call.scale,
            total_damage: call.total_damage,
        },
        op::SCREEN_FLASH => PlayerEffectOp::ScreenFlash { screen_flash: call.screen_flash, scale: call.scale },
        op::TELEFRAG => PlayerEffectOp::Telefrag { intensity: call.intensity },
        op::CONTINUOUS => PlayerEffectOp::Continuous {
            definition: continuous.as_deref().unwrap_or(&no_continuous),
            distance: call.distance,
        },
        op::CAMERA_MATRIX => PlayerEffectOp::CameraMatrix,
        op::GET_SCREEN_FLASH => PlayerEffectOp::GetScreenFlash,
        _ => return -1,
    };
    let mut state = call.state;
    call.difference_event = -1;
    call.difference_reason = 0;
    let output = if check {
        // SAFETY: the game's recording, as described
        let events = if call.events.is_null() || call.event_count == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) }
        };
        let checker = CheckHost { events, next: Cell::new(0), difference: Cell::new(None) };
        let output = player_effect(&operation, &inputs, &checker, &mut state, call.screen_flash_out);
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
        output
    } else {
        let live = LiveHost { callbacks: host.player_effects };
        player_effect(&operation, &inputs, &live, &mut state, call.screen_flash_out)
    };
    call.state = state;
    call.screen_flash_out = output.screen_flash;
    call.has_matrix = output.matrix.is_some() as u32;
    if let Some(m) = output.matrix {
        call.matrix = [
            m.scale,
            m.forward[0],
            m.forward[1],
            m.forward[2],
            m.left[0],
            m.left[1],
            m.left[2],
            m.up[0],
            m.up[1],
            m.up[2],
            m.position[0],
            m.position[1],
            m.position[2],
        ];
    }
    0
}

/// A player effect operation on the running game, in place of the game's.
/// Returns 0 (the state and output written to `call`), or -1 if it can't
/// run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid; its tables hold 1024 bytes each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_player_effect(call: *mut PlayerEffectCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of an operation the game ran: Rust's replay of it. Returns 0,
/// or -1.
///
/// # Safety
/// As [`halo_sim_player_effect`]; the events hold their count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_player_effect_check(call: *mut PlayerEffectCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
