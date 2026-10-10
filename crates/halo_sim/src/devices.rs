//! Device updates (`device_update`, `machine_update`) in Rust, on the
//! running game: power and position, doors, platforms and elevators are
//! `halo_map::devices`' (the bipeds near a machine found by its own
//! `objects_in_sphere`, on the live clusters); effects, device groups, teams
//! and translating objects are the game's, through devices.c's
//! `device_call` ([`DeviceCallbacks`]).
//!
//! In check mode the game's own update runs first, recording each call with
//! the device before and after, and each sphere's bipeds; Rust replays it
//! from the device as it was.

use std::cell::Cell;
use std::sync::Arc;

use halo_map::area_damage::{AreaObject, MAXIMUM_CLUSTERS_PER_STRUCTURE, objects_in_clusters_in_sphere};
use halo_map::devices::{DeviceBiped, DeviceCall, DeviceDefinition, DeviceHost, DeviceInputs, DeviceState, device_update, machine_update};

use crate::TAGS;

/// The game's side of device updates (halo_sim.c).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceCallbacks {
    /// A call: the device written to the game first, read back after.
    pub call: unsafe extern "C" fn(state: *mut DeviceState, call: *const DeviceCall) -> i32,
    /// A biped as a door or an elevator reads it: 1 if it is there.
    pub biped: unsafe extern "C" fn(index: u32, out: *mut DeviceBiped) -> i32,
}

/// The bipeds in a sphere, recorded in check mode as an event of its own.
pub const BIPEDS_IN_SPHERE: i32 = 6;

/// A call of the game's update, recorded in check mode (or a sphere's
/// bipeds: `call.a` the maximum, `call.b` the cluster, `call.f` the centre,
/// `radius`, and `bipeds[biped_first..][..biped_count]`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceEventC {
    pub call: DeviceCall,
    pub radius: f32,
    pub result: i32,
    pub before: DeviceState,
    pub after: DeviceState,
    pub biped_first: u32,
    pub biped_count: u32,
}

/// What a device's update reads, and where it leaves the device.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceCallC {
    pub state: DeviceState,
    /// 0 the device's part (`device_update`), 1 the machine's
    /// (`machine_update`).
    pub part: u32,
    pub structure_bsp_index: i32,
    /// Which breakable surfaces stand (32 bytes; null for every one).
    pub breakable_surfaces: *const u8,
    pub power_group_value: f32,
    pub position_group_value: f32,
    pub game_time: i32,
    pub has_elevator_node: u32,
    pub elevator_node_position: [f32; 3],
    /// In check mode: the game's calls and the bipeds its spheres found.
    pub events: *const DeviceEventC,
    pub event_count: u32,
    pub bipeds: *const DeviceBiped,
    pub biped_count: u32,
    /// In check mode: the first call that differs (-1 for none) and why
    /// (1 its kind, 2 what it was given, 3 the device before it, 4 more calls
    /// than the game's, 5 fewer); the device Rust leaves is compared by the
    /// game.
    pub difference_event: i32,
    pub difference_reason: i32,
    /// Whether the device's power or position moved (`device_update`'s
    /// result).
    pub changed: u32,
}

impl crate::Tags {
    fn device_definition(&mut self, datum: u32, machine: bool) -> Option<Arc<DeviceDefinition>> {
        let cache = &self.cache;
        self.device_definitions
            .entry((datum, machine))
            .or_insert_with(|| DeviceDefinition::load(cache, datum, machine).ok().map(Arc::new))
            .clone()
    }
}

/// The running game.
struct LiveHost {
    host: crate::world::Host,
    structure: Option<Arc<crate::Structure>>,
}

impl DeviceHost for LiveHost {
    fn call(&self, state: &mut DeviceState, call: &DeviceCall) -> i32 {
        // SAFETY: the game's callback
        unsafe { (self.host.devices.call)(state, call) }
    }

    fn bipeds_in_sphere(&self, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<DeviceBiped> {
        let Some(structure) = &self.structure else { return Vec::new() };
        let clusters = structure.clusters.clusters_in_sphere(&structure.collision, cluster, center, radius, MAXIMUM_CLUSTERS_PER_STRUCTURE);
        let callbacks = self.host.area_damage;
        let mut buffer = vec![0u32; halo_map::area_damage::MAXIMUM_OBJECTS_PER_MAP];
        let indices = objects_in_clusters_in_sphere(
            1,
            1,
            &clusters,
            &mut |cluster, partition, out| {
                // SAFETY: the game's callback, with a buffer of its size
                let count = unsafe { (callbacks.cluster_objects)(cluster, partition, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
                out.extend_from_slice(&buffer[..count.min(buffer.len())]);
            },
            &mut |index| {
                let mut object = AreaObject::default();
                // SAFETY: as above
                (unsafe { (callbacks.object)(index, &mut object) } != 0)
                    .then_some((object.object_type, object.bounding_center, object.bounding_radius))
            },
            center,
            radius,
            maximum,
        );
        indices
            .into_iter()
            .filter_map(|index| {
                let mut biped = DeviceBiped::default();
                // SAFETY: as above
                (unsafe { (self.host.devices.biped)(index, &mut biped) } != 0).then_some(biped)
            })
            .collect()
    }
}

/// The game's own update, as it recorded it.
struct CheckHost<'a> {
    events: &'a [DeviceEventC],
    bipeds: &'a [DeviceBiped],
    next: Cell<usize>,
    difference: Cell<Option<(usize, i32)>>,
}

impl CheckHost<'_> {
    fn differ(&self, event: usize, reason: i32) {
        if self.difference.get().is_none() {
            self.difference.set(Some((event, reason)));
        }
    }

    fn take(&self, kind: i32) -> Option<&DeviceEventC> {
        let index = self.next.get();
        let Some(event) = self.events.get(index) else {
            self.differ(index, 4);
            return None;
        };
        self.next.set(index + 1);
        if event.call.kind != kind {
            self.differ(index, 1);
        }
        Some(event)
    }
}

impl DeviceHost for CheckHost<'_> {
    fn call(&self, state: &mut DeviceState, call: &DeviceCall) -> i32 {
        let index = self.next.get();
        let Some(event) = self.take(call.kind) else { return 0 };
        let e = &event.call;
        if e.object_index != call.object_index || e.a != call.a || e.b != call.b || e.f.map(f32::to_bits) != call.f.map(f32::to_bits) {
            self.differ(index, 2);
        } else if event.before != *state {
            self.differ(index, 3);
        }
        *state = event.after;
        event.result
    }

    fn bipeds_in_sphere(&self, cluster: i16, center: [f32; 3], radius: f32, maximum: usize) -> Vec<DeviceBiped> {
        let index = self.next.get();
        let Some(event) = self.take(BIPEDS_IN_SPHERE) else { return Vec::new() };
        let e = &event.call;
        if e.a != maximum as i32
            || e.b != cluster as i32
            || e.f.map(f32::to_bits) != center.map(f32::to_bits)
            || event.radius.to_bits() != radius.to_bits()
        {
            self.differ(index, 2);
        }
        let first = event.biped_first as usize;
        self.bipeds.get(first..first + event.biped_count as usize).map_or_else(Vec::new, <[_]>::to_vec)
    }
}

/// # Safety
/// `pointer` must be null or point to `count` values.
unsafe fn slice<'a, T>(pointer: *const T, count: u32) -> &'a [T] {
    if pointer.is_null() || count == 0 {
        &[]
    } else {
        // SAFETY: as above
        unsafe { std::slice::from_raw_parts(pointer, count as usize) }
    }
}

fn run(call: &mut DeviceCallC, check: bool) -> i32 {
    let Some(host) = crate::world::host() else { return -1 };
    let machine = call.part == 1;
    let definition = {
        let mut tags = TAGS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(tags) = tags.as_mut() else { return -1 };
        tags.device_definition(call.state.definition_index, machine)
    };
    let Some(definition) = definition else { return -1 };
    let inputs = DeviceInputs {
        power_group_value: call.power_group_value,
        position_group_value: call.position_group_value,
        game_time: call.game_time,
        elevator_node_position: (call.has_elevator_node != 0).then_some(call.elevator_node_position),
    };
    let mut state = call.state;
    call.difference_event = -1;
    call.difference_reason = 0;
    let changed;
    if check {
        // SAFETY: the game's recording, as described
        let (events, bipeds) = unsafe { (slice(call.events, call.event_count), slice(call.bipeds, call.biped_count)) };
        let checker = CheckHost { events, bipeds, next: Cell::new(0), difference: Cell::new(None) };
        changed = if machine {
            machine_update(&inputs, &definition, &checker, &mut state);
            true
        } else {
            device_update(&inputs, &definition, &checker, &mut state)
        };
        if checker.difference.get().is_none() && checker.next.get() < events.len() {
            checker.differ(checker.next.get(), 5);
        }
        (call.difference_event, call.difference_reason) = checker.difference.get().map_or((-1, 0), |(e, r)| (e as i32, r));
    } else {
        let structure = if machine { crate::structure(call.structure_bsp_index, call.breakable_surfaces) } else { None };
        let live = LiveHost { host, structure };
        changed = if machine {
            machine_update(&inputs, &definition, &live, &mut state);
            true
        } else {
            device_update(&inputs, &definition, &live, &mut state)
        };
    }
    call.state = state;
    call.changed = changed as u32;
    0
}

/// A device's update (its device or machine part) on the running game, in
/// place of the game's. Returns 0 (the device's state written), or -1 if it
/// can't run (the game then runs its own; nothing was done).
///
/// # Safety
/// `call` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_device_update(call: *mut DeviceCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, false)
}

/// The check of an update the game ran: Rust's replay of it. Returns 0, or
/// -1.
///
/// # Safety
/// As [`halo_sim_device_update`]; the recording holds its counts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn halo_sim_device_update_check(call: *mut DeviceCallC) -> i32 {
    if call.is_null() {
        return -1;
    }
    // SAFETY: the caller's call
    run(unsafe { &mut *call }, true)
}
