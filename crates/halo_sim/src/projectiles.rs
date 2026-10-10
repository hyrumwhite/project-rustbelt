//! Projectiles (`projectile_update`) in Rust, on the running game: the
//! flight, its collisions and how it answers them are `halo_map`'s, and
//! what a projectile does to the world is the game's, called back at the
//! moments the C calls it ([`ProjectileCallbacks`]): its impact damage,
//! breakable surfaces, effects, the AI's notices, its moves (the flyby
//! sound, orientation, `object_translate` and contrail), sticking to what
//! it hit, and the needles a super-combining hit resets.
//!
//! Each callback is handed the update's state, which the game writes to
//! the projectile before it acts and reads back after, and the game's
//! random seed is the update's own at each call (the effects and damage
//! draw from it too).
//!
//! In check mode the game's own update runs, recording each of those calls
//! (with the random seed before and after); Rust then replays the update
//! from the state it began with, answered from the recording
//! ([`halo_sim_projectile_check`]), and the game compares where both left
//! the projectile.

use std::sync::Arc;

use halo_map::collision::{CollisionKind, CollisionResult, World};
use halo_map::projectile::{
    CollisionEffect, Guidance, ImpactDamage, MissingInput, ProjectileDefinition, ProjectileHost, ProjectileObject, ProjectileState, Random,
    projectile_update,
};

use crate::world::{GameObjects, HostObject};
use crate::{Structure, TAGS};

/// The game's `collision_result` (0x50 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CollisionResultC {
    pub kind: i16,
    pub pad0: i16,
    pub start_location_leaf: i32,
    pub start_location_cluster: i16,
    pub pad1: i16,
    pub location_leaf: i32,
    pub location_cluster: i16,
    pub pad2: i16,
    pub t: f32,
    pub point: [f32; 3],
    pub plane: [f32; 4],
    pub material_type: i16,
    pub pad3: u16,
    pub object_index: i32,
    pub region_index: i16,
    pub node_index: i16,
    pub bsp_index: i16,
    pub pad4: i16,
    pub surface_index: i32,
    pub plane_designator: i32,
    pub flags: u8,
    pub breakable_surface_index: u8,
    pub material_index: i16,
}

const _: () = assert!(std::mem::size_of::<CollisionResultC>() == 0x50);

/// A ray test's result as the game lays it out (its start location left
/// none: nothing a projectile's callbacks read).
fn collision_c(r: &CollisionResult) -> CollisionResultC {
    CollisionResultC {
        kind: match r.kind {
            None => -1,
            Some(CollisionKind::Media) => 0,
            Some(CollisionKind::Structure) => 2,
            Some(CollisionKind::Object) => 3,
        },
        start_location_leaf: -1,
        start_location_cluster: -1,
        location_leaf: r.location_leaf,
        location_cluster: r.location_cluster,
        t: r.t,
        point: r.point,
        plane: r.plane,
        material_type: r.material_type,
        object_index: r.object.map_or(-1, |o| o as i32),
        region_index: r.region,
        node_index: r.node,
        bsp_index: r.bsp,
        surface_index: r.surface,
        plane_designator: r.plane_designator,
        flags: r.surface_flags,
        breakable_surface_index: r.breakable_surface,
        material_index: r.material_index,
        ..Default::default()
    }
}

/// A guided step's inputs, as the game gives them.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct GuidanceC {
    pub projectile_center: [f32; 3],
    pub target_center: [f32; 3],
    pub target_center_of_mass: [f32; 3],
    pub guidance_scale: f32,
}

/// The game's side of a projectile's update (projectiles.c's
/// `projectile_rust_*`, through halo_sim.c, which writes the state to the
/// projectile first and reads it back after).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ProjectileCallbacks {
    pub timers_updated: unsafe extern "C" fn(projectile: u32, state: *mut ProjectileState),
    pub guidance: unsafe extern "C" fn(projectile: u32, state: *mut ProjectileState, out: *mut GuidanceC),
    /// A collision's response begins (the bridge records the seed).
    pub collision_begin: unsafe extern "C" fn(projectile: u32, state: *mut ProjectileState),
    pub impact_damage: unsafe extern "C" fn(
        projectile: u32,
        state: *mut ProjectileState,
        collision: *const CollisionResultC,
        damage_scale: f32,
        velocity: *const [f32; 3],
        material_type: *mut i16,
        material_effect_scale: *mut f32,
    ),
    pub breakable_surface_damage:
        unsafe extern "C" fn(projectile: u32, state: *mut ProjectileState, collision: *const CollisionResultC, velocity: *const [f32; 3]),
    /// `effect`: 0 the material response's (`potential` or default), 1 the
    /// detonation starting.
    pub effect: unsafe extern "C" fn(
        projectile: u32,
        state: *mut ProjectileState,
        effect: i32,
        material_type: i32,
        potential: i32,
        collision: *const CollisionResultC,
        direction: *const [f32; 3],
        scale: f32,
        material_effect_scale: f32,
    ),
    pub combining_reset: unsafe extern "C" fn(child: u32),
    pub attach:
        unsafe extern "C" fn(projectile: u32, state: *mut ProjectileState, new_position: *const [f32; 3], collision: *const CollisionResultC),
    pub impact_noise: unsafe extern "C" fn(projectile: u32, state: *mut ProjectileState, point: *const [f32; 3]),
    pub moved: unsafe extern "C" fn(
        projectile: u32,
        state: *mut ProjectileState,
        new_position: *const [f32; 3],
        new_velocity: *const [f32; 3],
        location_leaf: i32,
        location_cluster: i32,
        time_remaining: f32,
        collision_count: i32,
        ignore_object_index: i32,
    ),
}

/// A call of the game's update, recorded for the check
/// (`_halo_sim_projectile_event_*`).
pub mod event {
    pub const GUIDANCE: i16 = 1;
    pub const COLLISION: i16 = 2;
    pub const IMPACT_DAMAGE: i16 = 3;
    pub const BREAKABLE_SURFACE: i16 = 4;
    pub const EFFECT: i16 = 5;
    pub const DETONATION_STARTED: i16 = 6;
    pub const COMBINING_RESET: i16 = 7;
    pub const ATTACH: i16 = 8;
    pub const IMPACT_NOISE: i16 = 9;
    pub const MOVED: i16 = 10;
}

/// One recorded call: its kind, the random seed before and after, what it
/// was given (the object, the material type or breakable surface, whether
/// the potential response, the scales) and gave back (the damage's material
/// type and effect scale; a guided step's inputs; the state after a move or
/// attaching).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ProjectileEventC {
    pub kind: i16,
    pub material_type: i16,
    pub seed_before: u32,
    pub seed_after: u32,
    pub object_index: i32,
    pub potential: i32,
    pub scale: f32,
    pub material_effect_scale: f32,
    pub guidance: GuidanceC,
    pub game_time: i32,
    pub state: ProjectileState,
}

/// Why the check's replay differs (`difference_reason`).
pub mod difference {
    /// A call of another kind than the game's.
    pub const KIND: i32 = 1;
    /// The random seed going into a call.
    pub const SEED: i32 = 2;
    /// What a call was given.
    pub const INPUTS: i32 = 3;
    /// Where a move or attaching left the projectile.
    pub const STATE: i32 = 4;
    /// More calls than the game made.
    pub const EXTRA: i32 = 5;
    /// Fewer.
    pub const MISSING: i32 = 6;
    /// The replay stopped for an input it couldn't have.
    pub const INPUT: i32 = 7;
}

/// What a projectile update reads, and where it leaves the projectile.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ProjectileUpdateCall {
    pub projectile_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    /// Which breakable surfaces stand (the game's 32 bytes, read again
    /// after each callback).
    pub breakable_surfaces: *const u8,
    /// The state it begins with, and leaves.
    pub state: ProjectileState,
    /// The game's random seed (in check mode a copy).
    pub random_seed: *mut u32,
    /// The game's random directions (`random_vector_in_cone3d`'s table).
    pub directions: *const [f32; 3],
    pub direction_count: i32,
    /// `_periodic_function_wander`'s table (1024 bytes).
    pub wander_table: *const u8,
    pub game_time: i32,
    /// In check mode, the game's calls.
    pub events: *const ProjectileEventC,
    pub event_count: u32,
    // what the update left
    pub collision_count: i32,
    pub time_remaining: f32,
    /// In check mode, the first call that differs (-1 for none) and why.
    pub difference_event: i32,
    pub difference_reason: i32,
}

impl crate::Tags {
    fn projectile(&mut self, definition: u32) -> Option<Arc<ProjectileDefinition>> {
        let cache = &self.cache;
        self.projectiles.entry(definition).or_insert_with(|| ProjectileDefinition::load(cache, definition).ok().map(Arc::new)).clone()
    }
}

/// An object as a projectile's hit reads it, from the host.
fn projectile_object(objects: &GameObjects, index: u32) -> Option<ProjectileObject> {
    let mut o = std::mem::MaybeUninit::<HostObject>::uninit();
    // SAFETY: the host's callback fills the object
    if unsafe { (objects.host.object)(index, o.as_mut_ptr()) } == 0 {
        return None;
    }
    // SAFETY: filled above
    let o = unsafe { o.assume_init() };
    Some(ProjectileObject {
        index: o.object_index,
        definition_index: o.definition_index,
        kind: o.object_type,
        parent: o.parent,
        projectile_flags: o.projectile_flags,
    })
}

fn children(objects: &GameObjects, index: u32, out: &mut Vec<u32>) {
    let mut buffer = [0u32; 256];
    // SAFETY: the host's callback, with a buffer of its size
    let count = unsafe { (objects.host.children)(index, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    out.extend_from_slice(&buffer[..count.min(buffer.len())]);
}

/// The running game, as a projectile update's host.
struct LiveHost<'a> {
    callbacks: ProjectileCallbacks,
    objects: &'a GameObjects,
    structure: &'a Structure,
    call: &'a ProjectileUpdateCall,
    wander_table: Option<&'a [u8; 1024]>,
}

impl LiveHost<'_> {
    /// A callback, with the game's random seed the update's going in, and
    /// the update's the game's coming out (and the breakable surfaces read
    /// again).
    fn call<R>(&self, random: &mut Random<'_>, f: impl FnOnce() -> R) -> R {
        // SAFETY: the game's random seed
        unsafe { *self.call.random_seed = random.seed };
        let result = f();
        // SAFETY: as above
        random.seed = unsafe { *self.call.random_seed };
        self.refresh_breakable_surfaces();
        result
    }

    fn refresh_breakable_surfaces(&self) {
        if !self.call.breakable_surfaces.is_null() {
            // SAFETY: the game's 32 bytes of breakable surface flags
            self.structure.collision.breakable_surfaces.set(Some(unsafe { *(self.call.breakable_surfaces as *const [u8; 32]) }));
        }
    }
}

impl ProjectileHost for LiveHost<'_> {
    fn projectile_index(&self) -> u32 {
        self.call.projectile_index
    }

    fn definition_index(&self) -> u32 {
        self.call.definition_index
    }

    fn timers_updated(&mut self, state: &mut ProjectileState) {
        // SAFETY: the game's callback, with the update's state
        unsafe { (self.callbacks.timers_updated)(self.call.projectile_index, state) };
    }

    fn guidance(&mut self, state: &mut ProjectileState, _step: usize) -> Option<Guidance> {
        let mut g = GuidanceC::default();
        // SAFETY: the game's callback, filling `g`
        unsafe { (self.callbacks.guidance)(self.call.projectile_index, state, &mut g) };
        Some(Guidance {
            projectile_center: g.projectile_center,
            target_center: g.target_center,
            target_center_of_mass: g.target_center_of_mass,
            guidance_scale: g.guidance_scale,
            game_time: self.call.game_time,
        })
    }

    fn wander_table(&self) -> Option<&[u8; 1024]> {
        self.wander_table
    }

    fn collision_begin(&mut self, state: &mut ProjectileState, _collision_index: usize, random: &mut Random<'_>) {
        let projectile = self.call.projectile_index;
        let callback = self.callbacks.collision_begin;
        // SAFETY: the game's callback
        self.call(random, || unsafe { callback(projectile, state) });
    }

    fn impact_damage(
        &mut self,
        state: &mut ProjectileState,
        _collision_index: usize,
        collision: &CollisionResult,
        damage_scale: f32,
        velocity: [f32; 3],
        random: &mut Random<'_>,
    ) -> Option<ImpactDamage> {
        let c = collision_c(collision);
        let mut material_type = -1i16;
        let mut material_effect_scale = 0.0f32;
        let projectile = self.call.projectile_index;
        let callback = self.callbacks.impact_damage;
        // SAFETY: the game's callback
        self.call(random, || unsafe {
            callback(projectile, state, &c, damage_scale, &velocity, &mut material_type, &mut material_effect_scale)
        });
        Some(ImpactDamage { material_type, material_effect_scale })
    }

    fn object(&self, index: u32) -> Option<ProjectileObject> {
        projectile_object(self.objects, index)
    }

    fn children(&self, index: u32, out: &mut Vec<u32>) {
        children(self.objects, index, out);
    }

    fn breakable_surface_damage(&mut self, state: &mut ProjectileState, collision: &CollisionResult, velocity: [f32; 3], random: &mut Random<'_>) {
        let c = collision_c(collision);
        let projectile = self.call.projectile_index;
        let callback = self.callbacks.breakable_surface_damage;
        // SAFETY: the game's callback
        self.call(random, || unsafe { callback(projectile, state, &c, &velocity) });
    }

    fn effect(
        &mut self,
        state: &mut ProjectileState,
        effect: CollisionEffect,
        collision: &CollisionResult,
        direction: [f32; 3],
        scale: f32,
        material_effect_scale: f32,
        random: &mut Random<'_>,
    ) {
        let c = collision_c(collision);
        let (which, material_type, potential) = match effect {
            CollisionEffect::Response { material_type, potential } => (0, material_type as i32, potential as i32),
            CollisionEffect::DetonationStarted => (1, -1, 0),
        };
        let projectile = self.call.projectile_index;
        let callback = self.callbacks.effect;
        // SAFETY: the game's callback
        self.call(random, || unsafe {
            callback(projectile, state, which, material_type, potential, &c, &direction, scale, material_effect_scale)
        });
    }

    fn combining_reset(&mut self, child: u32) {
        // SAFETY: the game's callback
        unsafe { (self.callbacks.combining_reset)(child) };
    }

    fn attach(
        &mut self,
        _world: &World<'_>,
        state: &mut ProjectileState,
        collision: &CollisionResult,
        new_position: [f32; 3],
        random: &mut Random<'_>,
    ) -> Result<Option<u32>, MissingInput> {
        let c = collision_c(collision);
        let projectile = self.call.projectile_index;
        let callback = self.callbacks.attach;
        // SAFETY: the game's callback
        self.call(random, || unsafe { callback(projectile, state, &new_position, &c) });
        Ok(if collision.kind == Some(CollisionKind::Object) { collision.object } else { None })
    }

    fn impact_noise(&mut self, state: &mut ProjectileState, point: [f32; 3], random: &mut Random<'_>) {
        let projectile = self.call.projectile_index;
        let callback = self.callbacks.impact_noise;
        // SAFETY: the game's callback
        self.call(random, || unsafe { callback(projectile, state, &point) });
    }

    fn moved(
        &mut self,
        state: &mut ProjectileState,
        new_position: [f32; 3],
        new_velocity: [f32; 3],
        location: (i32, i16),
        time_remaining: f32,
        collision_count: usize,
        ignore_object_index: i32,
        random: &mut Random<'_>,
    ) {
        let projectile = self.call.projectile_index;
        let callback = self.callbacks.moved;
        // SAFETY: the game's callback
        self.call(random, || unsafe {
            callback(
                projectile,
                state,
                &new_position,
                &new_velocity,
                location.0,
                location.1 as i32,
                time_remaining,
                collision_count as i32,
                ignore_object_index,
            )
        });
    }
}

/// The game's own update, as it recorded it, as a projectile update's
/// host: each call answered from the recording, and compared with it.
struct CheckHost<'a> {
    objects: &'a GameObjects,
    call: &'a ProjectileUpdateCall,
    events: &'a [ProjectileEventC],
    next: usize,
    wander_table: Option<&'a [u8; 1024]>,
    difference: Option<(usize, i32)>,
}

impl CheckHost<'_> {
    fn differ(&mut self, event: usize, reason: i32) {
        if self.difference.is_none() {
            self.difference = Some((event, reason));
        }
    }

    /// The next recorded call, which should be of `kind` and begin with the
    /// update's random seed; the seed it left.
    fn expect(&mut self, kind: i16, random: &mut Random<'_>) -> Option<(usize, ProjectileEventC)> {
        let index = self.next;
        let Some(&event) = self.events.get(index) else {
            self.differ(index, difference::EXTRA);
            return None;
        };
        self.next += 1;
        if event.kind != kind {
            self.differ(index, difference::KIND);
        } else if event.seed_before != random.seed {
            self.differ(index, difference::SEED);
        }
        random.seed = event.seed_after;
        Some((index, event))
    }
}

fn bits3(v: [f32; 3]) -> [u32; 3] {
    v.map(f32::to_bits)
}

impl ProjectileHost for CheckHost<'_> {
    fn projectile_index(&self) -> u32 {
        self.call.projectile_index
    }

    fn definition_index(&self) -> u32 {
        self.call.definition_index
    }

    fn guidance(&mut self, _state: &mut ProjectileState, _step: usize) -> Option<Guidance> {
        // (the steering draws nothing: the seed it sees is the update's)
        let mut random = Random { seed: self.events.get(self.next).map_or(0, |e| e.seed_before), directions: &[] };
        let (_, event) = self.expect(event::GUIDANCE, &mut random)?;
        let g = event.guidance;
        Some(Guidance {
            projectile_center: g.projectile_center,
            target_center: g.target_center,
            target_center_of_mass: g.target_center_of_mass,
            guidance_scale: g.guidance_scale,
            game_time: event.game_time,
        })
    }

    fn wander_table(&self) -> Option<&[u8; 1024]> {
        self.wander_table
    }

    fn collision_begin(&mut self, _state: &mut ProjectileState, _collision_index: usize, random: &mut Random<'_>) {
        self.expect(event::COLLISION, random);
    }

    fn impact_damage(
        &mut self,
        _state: &mut ProjectileState,
        _collision_index: usize,
        collision: &CollisionResult,
        damage_scale: f32,
        _velocity: [f32; 3],
        random: &mut Random<'_>,
    ) -> Option<ImpactDamage> {
        let (index, event) = self.expect(event::IMPACT_DAMAGE, random)?;
        if event.object_index != collision.object.map_or(-1, |o| o as i32) || event.scale.to_bits() != damage_scale.to_bits() {
            self.differ(index, difference::INPUTS);
        }
        Some(ImpactDamage { material_type: event.material_type, material_effect_scale: event.material_effect_scale })
    }

    fn object(&self, index: u32) -> Option<ProjectileObject> {
        projectile_object(self.objects, index)
    }

    fn children(&self, index: u32, out: &mut Vec<u32>) {
        // (not the projectile itself, which the game's update stuck there)
        children(self.objects, index, out);
        out.retain(|&child| child != self.call.projectile_index);
    }

    fn breakable_surface_damage(&mut self, _state: &mut ProjectileState, collision: &CollisionResult, _velocity: [f32; 3], random: &mut Random<'_>) {
        if let Some((index, event)) = self.expect(event::BREAKABLE_SURFACE, random)
            && event.material_type != collision.breakable_surface as i16
        {
            self.differ(index, difference::INPUTS);
        }
    }

    fn effect(
        &mut self,
        _state: &mut ProjectileState,
        effect: CollisionEffect,
        _collision: &CollisionResult,
        _direction: [f32; 3],
        scale: f32,
        material_effect_scale: f32,
        random: &mut Random<'_>,
    ) {
        let (kind, material_type, potential) = match effect {
            CollisionEffect::Response { material_type, potential } => (event::EFFECT, material_type, potential as i32),
            CollisionEffect::DetonationStarted => (event::DETONATION_STARTED, -1, 0),
        };
        if let Some((index, event)) = self.expect(kind, random)
            && (event.material_type != material_type
                || event.potential != potential
                || event.scale.to_bits() != scale.to_bits()
                || event.material_effect_scale.to_bits() != material_effect_scale.to_bits())
        {
            self.differ(index, difference::INPUTS);
        }
    }

    fn combining_reset(&mut self, child: u32) {
        let mut random = Random { seed: self.events.get(self.next).map_or(0, |e| e.seed_before), directions: &[] };
        if let Some((index, event)) = self.expect(event::COMBINING_RESET, &mut random)
            && event.object_index != child as i32
        {
            self.differ(index, difference::INPUTS);
        }
    }

    fn attach(
        &mut self,
        world: &World<'_>,
        state: &mut ProjectileState,
        collision: &CollisionResult,
        new_position: [f32; 3],
        random: &mut Random<'_>,
    ) -> Result<Option<u32>, MissingInput> {
        let attached = halo_map::projectile::attach(world, state, collision, new_position)?;
        if let Some((index, event)) = self.expect(event::ATTACH, random)
            && (bits3(event.state.position) != bits3(state.position)
                || event.state.parent_object_index != state.parent_object_index
                || event.state.location_cluster != state.location_cluster)
        {
            self.differ(index, difference::STATE);
        }
        Ok(attached)
    }

    fn impact_noise(&mut self, _state: &mut ProjectileState, _point: [f32; 3], random: &mut Random<'_>) {
        self.expect(event::IMPACT_NOISE, random);
    }

    fn moved(
        &mut self,
        state: &mut ProjectileState,
        new_position: [f32; 3],
        new_velocity: [f32; 3],
        location: (i32, i16),
        _time_remaining: f32,
        _collision_count: usize,
        _ignore_object_index: i32,
        random: &mut Random<'_>,
    ) {
        state.position = new_position;
        if location.1 != -1 {
            (state.location_leaf, state.location_cluster) = location;
            state.object_flags &= !halo_map::projectile::object_flags::OUTSIDE_OF_MAP;
        } else {
            state.object_flags |= halo_map::projectile::object_flags::OUTSIDE_OF_MAP;
        }
        state.velocity = new_velocity;
        if let Some((index, event)) = self.expect(event::MOVED, random)
            && (bits3(event.state.position) != bits3(state.position)
                || bits3(event.state.velocity) != bits3(state.velocity)
                || event.state.location_leaf != state.location_leaf
                || event.state.location_cluster != state.location_cluster)
        {
            self.differ(index, difference::STATE);
        }
    }
}

/// What both entry points read: the structure, the definition, the random
/// directions and the wander table.
struct Prepared {
    structure: Arc<Structure>,
    definition: Arc<ProjectileDefinition>,
    objects: GameObjects,
}

fn prepare(call: &ProjectileUpdateCall) -> Option<Prepared> {
    if call.random_seed.is_null() || call.directions.is_null() || call.direction_count <= 0 {
        return None;
    }
    let host = crate::world::host()?;
    let structure = crate::structure(call.structure_bsp_index, call.breakable_surfaces)?;
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        tags.as_mut()?.projectile(call.definition_index)?
    };
    // a guided projectile steers by the wander table
    if call.state.target_object_index != -1 && definition.guided_angular_velocity > 0.0 && call.wander_table.is_null() {
        return None;
    }
    Some(Prepared { structure, definition, objects: GameObjects { host } })
}

/// One projectile update on the running game, in place of the game's.
/// Returns 0 (the state, random seed, collision count and time remaining
/// written), or -1 if it can't run (the game then runs its own; nothing
/// was done), or -2 if it stopped for an input it couldn't have.
///
/// # Safety
/// `call` must be valid, its pointers the game's as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_projectile_update(call: *mut ProjectileUpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    let Some(prepared) = prepare(call) else { return -1 };
    let Some(callbacks) = crate::world::host().map(|h| h.projectile) else { return -1 };
    // SAFETY: the game's tables, as described
    let directions = unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) };
    // SAFETY: as above
    let wander_table = (!call.wander_table.is_null()).then(|| unsafe { &*(call.wander_table as *const [u8; 1024]) });
    let world = World {
        collision: &prepared.structure.collision,
        clusters: &prepared.structure.clusters,
        objects: &[],
        source: Some(&prepared.objects),
    };
    let mut state = call.state;
    // SAFETY: the game's random seed
    let mut random = Random { seed: unsafe { *call.random_seed }, directions };
    let snapshot = *call;
    let mut host = LiveHost { callbacks, objects: &prepared.objects, structure: &prepared.structure, call: &snapshot, wander_table };
    let result = projectile_update(&world, &prepared.definition, &mut state, &mut random, &mut host);
    // SAFETY: as above
    unsafe { *call.random_seed = random.seed };
    call.state = state;
    match result {
        Ok(summary) => {
            call.collision_count = summary.collisions as i32;
            call.time_remaining = summary.time_remaining;
            0
        }
        Err(_) => -2,
    }
}

/// The check of a projectile update the game ran: Rust's replay of it from
/// the state it began with (`call.state`, and the seed at
/// `call.random_seed`, a copy), each of the game's calls answered from its
/// recording (`call.events`). Returns 0 (the state Rust left, its
/// collision count, time remaining, and the first call that differs
/// written), or -1 if it can't run.
///
/// # Safety
/// `call` must be valid, its pointers the game's as described, and
/// `call.events` hold `call.event_count` calls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_projectile_check(call: *mut ProjectileUpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    let call = unsafe { &mut *call };
    let Some(prepared) = prepare(call) else { return -1 };
    // SAFETY: the game's tables and the recording, as described
    let directions = unsafe { std::slice::from_raw_parts(call.directions, call.direction_count as usize) };
    // SAFETY: as above
    let wander_table = (!call.wander_table.is_null()).then(|| unsafe { &*(call.wander_table as *const [u8; 1024]) });
    let events: &[ProjectileEventC] =
        if call.events.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(call.events, call.event_count as usize) } };
    let world = World {
        collision: &prepared.structure.collision,
        clusters: &prepared.structure.clusters,
        objects: &[],
        source: Some(&prepared.objects),
    };
    let mut state = call.state;
    // SAFETY: the copy of the seed it began with
    let mut random = Random { seed: unsafe { *call.random_seed }, directions };
    let snapshot = *call;
    let mut host = CheckHost { objects: &prepared.objects, call: &snapshot, events, next: 0, wander_table, difference: None };
    let result = projectile_update(&world, &prepared.definition, &mut state, &mut random, &mut host);
    // SAFETY: as above
    unsafe { *call.random_seed = random.seed };
    call.state = state;
    match result {
        Ok(summary) => {
            call.collision_count = summary.collisions as i32;
            call.time_remaining = summary.time_remaining;
            if host.difference.is_none() && host.next < events.len() {
                host.differ(host.next, difference::MISSING);
            }
        }
        Err(_) => host.differ(host.next, difference::INPUT),
    }
    (call.difference_event, call.difference_reason) = host.difference.map_or((-1, 0), |(e, r)| (e as i32, r));
    0
}
