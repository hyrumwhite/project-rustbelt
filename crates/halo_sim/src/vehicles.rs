//! Vehicles' physics (`physics_update`) in Rust, on the running game: the
//! forces at each mass point (their ground planes from the live objects'
//! collision features), the integration and the freeze against
//! penetrating the world are `halo_map`'s. The game moves the vehicle
//! (`object_set_position`, `object_translate`) and does its unit
//! collisions (`physics_compute_unit_collisions`, which push the bipeds
//! and vehicles it hits), called back with the update's state.
//!
//! Vehicles' control (`vehicle_update`, up to its falling damage) runs in
//! Rust too: its steering, speed and turn, its type's control code, its
//! physics as above, its suspensions and airborne ticks. Its effects and
//! sounds, and the move of a vehicle coasting to a stop, are the game's.
//!
//! In check mode the game's own update runs first; Rust then replays it
//! from the state it began with, on the live world, as the probe replays
//! a recorded one: with the bounding sphere centre the game's move left it
//! and the other vehicles its unit collisions considered.

use std::sync::Arc;

use halo_map::biped::BipedObjects;
use halo_map::collision::World;
use halo_map::vehicle_physics::{
    MassPoint, MaterialPhysics, OtherVehicle, PhysicsDefinition, PoweredMassPoint, VehiclePhysicsHost, VehiclePhysicsInputs,
    VehiclePhysicsState, VehiclePhysicsWorld, load_material_physics, physics_update,
};
use halo_map::vehicle_update::{
    VehicleDefinition, VehicleUpdateHost, VehicleUpdateInputs, VehicleUpdateState, VehicleUpdateWorld, vehicle_update,
};

use crate::TAGS;
use crate::world::GameObjects;

/// The game's side of a vehicle's physics (halo_sim.c, which writes the
/// state to the vehicle first and reads it back after).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehiclePhysicsCallbacks {
    pub set_position:
        unsafe extern "C" fn(vehicle: u32, state: *mut VehiclePhysicsState, position: *const [f32; 3], forward: *const [f32; 3], up: *const [f32; 3]),
    pub translate: unsafe extern "C" fn(vehicle: u32, state: *mut VehiclePhysicsState, new_position: *const [f32; 3]),
    pub unit_collisions: unsafe extern "C" fn(vehicle: u32, state: *mut VehiclePhysicsState),
    /// An update begins (with its vehicle code's powered mass points, null
    /// for none, and magic force and torque, null for none) and ends (with
    /// its mass points), for the game's recording of it.
    pub begin: unsafe extern "C" fn(
        vehicle: u32,
        state: *mut VehiclePhysicsState,
        powered: *const PoweredMassPointC,
        powered_count: i32,
        magic: *const [[f32; 3]; 2],
        old: i32,
    ),
    pub end: unsafe extern "C" fn(vehicle: u32, state: *mut VehiclePhysicsState, mass_points: *const MassPoint, count: u32),
}

/// Another vehicle the game's unit collisions considered (check mode).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct OtherVehicleC {
    pub object_index: u32,
    pub definition_index: u32,
    pub at_rest: u32,
    pub origin: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
}

/// The game's `powered_mass_point_datum` (96 bytes): what the vehicle's
/// code hands its physics.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PoweredMassPointC {
    pub values: [f32; 7],
    pub rotation: [f32; 4],
    pub rotation_matrix: [f32; 13],
}

const _: () = assert!(std::mem::size_of::<PoweredMassPointC>() == 96);

/// What a vehicle's physics update reads, and where it leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehiclePhysicsCall {
    pub vehicle_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub breakable_surfaces: *const u8,
    /// The state it begins with, and leaves.
    pub state: VehiclePhysicsState,
    /// The vehicle code's powered mass points (null for none), and its
    /// magic force and torque.
    pub powered: *const PoweredMassPointC,
    pub powered_count: i32,
    pub has_magic: u32,
    pub magic_force: [f32; 3],
    pub magic_torque: [f32; 3],
    /// In check mode: the bounding sphere centre the game's move left it,
    /// and the other vehicles its unit collisions considered.
    pub bounding_center_after: [f32; 3],
    pub others: *const OtherVehicleC,
    pub other_count: u32,
    /// Its mass points (the game's array, written).
    pub mass_points: *mut MassPoint,
    pub mass_point_count: u32,
}

impl crate::Tags {
    fn physics_definition(&mut self, definition: u32) -> Option<Arc<PhysicsDefinition>> {
        let cache = &self.cache;
        self.physics_definitions
            .entry(definition)
            .or_insert_with(|| PhysicsDefinition::load_for_object(cache, definition).ok().flatten().map(Arc::new))
            .clone()
    }

    fn material_physics(&mut self) -> Arc<Vec<MaterialPhysics>> {
        let cache = &self.cache;
        self.material_physics.get_or_insert_with(|| Arc::new(load_material_physics(cache).unwrap_or_default())).clone()
    }
}

/// The running game, moving the vehicle and doing its unit collisions.
struct LiveHost {
    callbacks: VehiclePhysicsCallbacks,
    vehicle_index: u32,
}

impl VehiclePhysicsHost for LiveHost {
    fn set_position(&self, state: &mut VehiclePhysicsState, position: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
        // SAFETY: the game's callback, with the update's state
        unsafe { (self.callbacks.set_position)(self.vehicle_index, state, &position, &forward, &up) };
    }

    fn translate(&self, state: &mut VehiclePhysicsState, new_position: [f32; 3]) {
        // SAFETY: as above
        unsafe { (self.callbacks.translate)(self.vehicle_index, state, &new_position) };
    }

    fn unit_collisions(&self, state: &mut VehiclePhysicsState) {
        // SAFETY: as above
        unsafe { (self.callbacks.unit_collisions)(self.vehicle_index, state) };
    }

    fn begin(&self, state: &mut VehiclePhysicsState, inputs: &VehiclePhysicsInputs<'_>, old: bool) {
        let powered: Option<Vec<PoweredMassPointC>> = inputs.powered.map(|points| {
            points
                .iter()
                .map(|p| PoweredMassPointC {
                    values: [
                        p.ground_friction_velocity,
                        p.water_friction_velocity,
                        p.air_friction_velocity,
                        p.water_lift_ratio,
                        p.air_lift_ratio,
                        p.thrust_fraction,
                        p.antigrav_fraction,
                    ],
                    rotation: p.rotation,
                    rotation_matrix: [0.0; 13],
                })
                .collect()
        });
        let magic = inputs.magic.map(|(force, torque)| [force, torque]);
        let (powered_pointer, powered_count) = powered.as_ref().map_or((std::ptr::null(), 0), |p| (p.as_ptr(), p.len() as i32));
        let magic_pointer = magic.as_ref().map_or(std::ptr::null(), |m| m as *const [[f32; 3]; 2]);
        // SAFETY: as above
        unsafe { (self.callbacks.begin)(self.vehicle_index, state, powered_pointer, powered_count, magic_pointer, old as i32) };
    }

    fn end(&self, state: &mut VehiclePhysicsState, mass_points: &[MassPoint]) {
        // SAFETY: as above
        unsafe { (self.callbacks.end)(self.vehicle_index, state, mass_points.as_ptr(), mass_points.len() as u32) };
    }
}

fn run(call: &mut VehiclePhysicsCall, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let Some(structure) = crate::structure(call.structure_bsp_index, call.breakable_surfaces) else { return -1 };
    let (physics, materials, others) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        let Some(physics) = tags.physics_definition(call.definition_index) else { return -1 };
        let others: &[OtherVehicleC] = if !check || call.others.is_null() {
            &[]
        } else {
            // SAFETY: the game's recording, `other_count` of them
            unsafe { std::slice::from_raw_parts(call.others, call.other_count as usize) }
        };
        let mut placed = Vec::new();
        for other in others {
            let Some(other_physics) = tags.physics_definition(other.definition_index) else { return -1 };
            placed.push(OtherVehicle {
                index: other.object_index,
                at_rest: other.at_rest != 0,
                physics: other_physics,
                origin: other.origin,
                forward: other.forward,
                up: other.up,
            });
        }
        (physics, tags.material_physics(), placed)
    };
    if call.mass_points.is_null() || (call.mass_point_count as usize) < physics.mass_points.len() {
        return -1;
    }
    let powered: Option<Vec<PoweredMassPoint>> = (!call.powered.is_null()).then(|| {
        // SAFETY: the game's powered mass points, `powered_count` of them
        unsafe { std::slice::from_raw_parts(call.powered, call.powered_count.max(0) as usize) }
            .iter()
            .map(|p| PoweredMassPoint {
                ground_friction_velocity: p.values[0],
                water_friction_velocity: p.values[1],
                air_friction_velocity: p.values[2],
                water_lift_ratio: p.values[3],
                air_lift_ratio: p.values[4],
                thrust_fraction: p.values[5],
                antigrav_fraction: p.values[6],
                rotation: p.rotation,
            })
            .collect()
    });
    let source = GameObjects { host };
    let world = World { collision: &structure.collision, clusters: &structure.clusters, objects: &[], source: Some(&source) };
    let live = LiveHost { callbacks: host.vehicle_physics, vehicle_index: call.vehicle_index };
    let env = VehiclePhysicsWorld {
        world: &world,
        feature_objects: BipedObjects::Live { clusters: &structure.clusters, source: &source },
        materials: &materials,
        others: &others,
        host: if check { None } else { Some(&live) },
    };
    let inputs = VehiclePhysicsInputs {
        index: call.vehicle_index,
        powered: powered.as_deref(),
        magic: (call.has_magic != 0).then_some((call.magic_force, call.magic_torque)),
        bounding_center_after: call.bounding_center_after,
    };
    let mut state = call.state;
    let outcome = physics_update(&physics, &mut state, &inputs, &env);
    call.state = state;
    for (index, mass_point) in outcome.mass_points.iter().enumerate().take(call.mass_point_count as usize) {
        // SAFETY: the game's array, `mass_point_count` of them
        unsafe { *call.mass_points.add(index) = *mass_point };
    }
    0
}

/// A vehicle's physics update on the running game, in place of the game's
/// (the vehicle moved and its unit collisions done by the game, called
/// back). Returns 0 (the state and mass points written), or -1 if it can't
/// run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its pointers the game's as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_vehicle_physics_update(call: *mut VehiclePhysicsCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of a vehicle's physics update the game ran: Rust's replay of
/// it from the state it began with, with what the game's move and unit
/// collisions met. Returns 0 (the state and mass points written: in check
/// mode the game passes copies), or -1.
///
/// # Safety
/// As [`halo_sim_vehicle_physics_update`]; `call.others` holds
/// `call.other_count` vehicles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_vehicle_physics_check(call: *mut VehiclePhysicsCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}

/// The game's side of a vehicle's update (halo_sim.c, which writes the
/// state to the vehicle first and reads it back after).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehicleUpdateCallbacks {
    pub set_position:
        unsafe extern "C" fn(vehicle: u32, state: *mut VehicleUpdateState, position: *const [f32; 3], forward: *const [f32; 3], up: *const [f32; 3]),
    pub suspension_shift: unsafe extern "C" fn(vehicle: u32, state: *mut VehicleUpdateState, maximum_shift: f32) -> i32,
    pub slipping_effects: unsafe extern "C" fn(vehicle: u32, state: *mut VehicleUpdateState, mass_points: *const MassPoint, count: u32),
    pub crashing_effects: unsafe extern "C" fn(
        vehicle: u32,
        state: *mut VehicleUpdateState,
        previous_velocity: *const [f32; 3],
        mass_points: *const MassPoint,
        count: u32,
    ),
    pub thruster_effect: unsafe extern "C" fn(vehicle: u32, state: *mut VehicleUpdateState, pelican: i32),
}

/// What a vehicle's update reads (`vehicle_update`, up to its falling
/// damage and animation), and where it leaves it.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VehicleUpdateCall {
    pub vehicle_index: u32,
    pub definition_index: u32,
    pub structure_bsp_index: i32,
    pub breakable_surfaces: *const u8,
    /// The state it begins with, and leaves.
    pub state: VehicleUpdateState,
    pub throttle: [f32; 3],
    pub control_flags: u32,
    pub desired_facing: [f32; 3],
    pub seat_power: [f32; 2],
    pub driven_by_ai: u32,
    pub vehicle_floor: f32,
    pub vehicle_ceiling: f32,
    /// In check mode: the bounding sphere centre the game's update left it,
    /// the powered mass points its control code handed the physics, and
    /// the other vehicles its physics' unit collisions considered.
    pub bounding_center_after: [f32; 3],
    pub powered_base: *const PoweredMassPointC,
    pub powered_base_count: i32,
    pub others: *const OtherVehicleC,
    pub other_count: u32,
}

impl crate::Tags {
    fn vehicle_definition(&mut self, definition: u32) -> Option<Arc<VehicleDefinition>> {
        let cache = &self.cache;
        self.vehicle_definitions.entry(definition).or_insert_with(|| VehicleDefinition::load(cache, definition).ok().map(Arc::new)).clone()
    }
}

/// The running game, making a vehicle update's effects and sounds.
struct LiveUpdateHost {
    callbacks: VehicleUpdateCallbacks,
    vehicle_index: u32,
}

impl VehicleUpdateHost for LiveUpdateHost {
    fn set_position(&self, state: &mut VehicleUpdateState, position: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
        // SAFETY: the game's callback, with the update's state
        unsafe { (self.callbacks.set_position)(self.vehicle_index, state, &position, &forward, &up) };
    }

    fn suspension_shift(&self, state: &mut VehicleUpdateState, maximum_shift: f32) -> bool {
        // SAFETY: as above
        unsafe { (self.callbacks.suspension_shift)(self.vehicle_index, state, maximum_shift) != 0 }
    }

    fn slipping_effects(&self, state: &mut VehicleUpdateState, mass_points: &[MassPoint]) {
        // SAFETY: as above
        unsafe { (self.callbacks.slipping_effects)(self.vehicle_index, state, mass_points.as_ptr(), mass_points.len() as u32) };
    }

    fn crashing_effects(&self, state: &mut VehicleUpdateState, previous_velocity: [f32; 3], mass_points: &[MassPoint]) {
        // SAFETY: as above
        unsafe {
            (self.callbacks.crashing_effects)(self.vehicle_index, state, &previous_velocity, mass_points.as_ptr(), mass_points.len() as u32)
        };
    }

    fn thruster_effect(&self, state: &mut VehicleUpdateState, pelican: bool) {
        // SAFETY: as above
        unsafe { (self.callbacks.thruster_effect)(self.vehicle_index, state, pelican as i32) };
    }
}

fn powered_points(powered: *const PoweredMassPointC, count: i32) -> Vec<PoweredMassPoint> {
    if powered.is_null() {
        return Vec::new();
    }
    // SAFETY: the game's powered mass points, `count` of them
    unsafe { std::slice::from_raw_parts(powered, count.max(0) as usize) }
        .iter()
        .map(|p| PoweredMassPoint {
            ground_friction_velocity: p.values[0],
            water_friction_velocity: p.values[1],
            air_friction_velocity: p.values[2],
            water_lift_ratio: p.values[3],
            air_lift_ratio: p.values[4],
            thrust_fraction: p.values[5],
            antigrav_fraction: p.values[6],
            rotation: p.rotation,
        })
        .collect()
}

fn run_update(call: &mut VehicleUpdateCall, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let Some(structure) = crate::structure(call.structure_bsp_index, call.breakable_surfaces) else { return -1 };
    let (definition, physics, materials, others) = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        let Some(definition) = tags.vehicle_definition(call.definition_index) else { return -1 };
        let physics = tags.physics_definition(call.definition_index);
        let others: &[OtherVehicleC] = if !check || call.others.is_null() {
            &[]
        } else {
            // SAFETY: the game's recording, `other_count` of them
            unsafe { std::slice::from_raw_parts(call.others, call.other_count as usize) }
        };
        let mut placed = Vec::new();
        for other in others {
            let Some(other_physics) = tags.physics_definition(other.definition_index) else { return -1 };
            placed.push(OtherVehicle {
                index: other.object_index,
                at_rest: other.at_rest != 0,
                physics: other_physics,
                origin: other.origin,
                forward: other.forward,
                up: other.up,
            });
        }
        (definition, physics, tags.material_physics(), placed)
    };
    let powered_base = if check { powered_points(call.powered_base, call.powered_base_count) } else { Vec::new() };
    let source = GameObjects { host };
    let world = World { collision: &structure.collision, clusters: &structure.clusters, objects: &[], source: Some(&source) };
    let physics_host = LiveHost { callbacks: host.vehicle_physics, vehicle_index: call.vehicle_index };
    let update_host = LiveUpdateHost { callbacks: host.vehicle_update, vehicle_index: call.vehicle_index };
    let env = VehicleUpdateWorld {
        physics: Some(VehiclePhysicsWorld {
            world: &world,
            feature_objects: BipedObjects::Live { clusters: &structure.clusters, source: &source },
            materials: &materials,
            others: &others,
            host: if check { None } else { Some(&physics_host) },
        }),
        world: &world,
        host: if check { None } else { Some(&update_host) },
    };
    let inputs = VehicleUpdateInputs {
        index: call.vehicle_index,
        parented: false,
        throttle: call.throttle,
        control_flags: call.control_flags,
        desired_facing: call.desired_facing,
        seat_power: call.seat_power,
        driven_by_ai: call.driven_by_ai != 0,
        vehicle_floor: call.vehicle_floor,
        vehicle_ceiling: call.vehicle_ceiling,
        bounding_center_after: call.bounding_center_after,
        powered_base: &powered_base,
    };
    let mut state = call.state;
    vehicle_update(&definition, physics.as_deref(), &mut state, &inputs, &env);
    call.state = state;
    0
}

/// A vehicle's update on the running game, in place of the game's, up to
/// its falling damage (its physics, effects and sounds the game's, called
/// back). Returns 0 (the state written), or -1 if it can't run (the game
/// then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid, its pointers the game's as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_vehicle_update(call: *mut VehicleUpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_update(unsafe { &mut *call }, false)
}

/// The check of a vehicle update the game ran: Rust's replay of it from
/// the state it began with, with what the game's update met. Returns 0
/// (the state written), or -1.
///
/// # Safety
/// As [`halo_sim_vehicle_update`]; `call.powered_base` and `call.others`
/// hold their counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_vehicle_update_check(call: *mut VehicleUpdateCall) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run_update(unsafe { &mut *call }, true)
}
