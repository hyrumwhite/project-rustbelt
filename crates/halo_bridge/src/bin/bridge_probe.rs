//! Prints what a running OpenCE shares through its Bevy bridge, once a second.
//!
//! ```text
//! bridge_probe [seconds] [walk] [listen] [pad] [latency] [fire] [grenades] [<map file>]
//! ```
//!
//! `walk` also forwards input: W held (and the mouse turning slowly) for the
//! second half, to check the game takes input through the bridge. `listen`
//! moves the game's listener to a point of its own for the second half (the
//! game logs the change). `pad` forwards a gamepad, its left stick pushed
//! forward for the second half. `latency` turns the view with the mouse twice
//! a second and reports how long the game's shared camera takes to follow.
//! `fire` holds the left mouse button for the second half. A map file
//! checks halo_map's collision code against the rays the game tests
//! (start the game with HALO_BEVY_BRIDGE_RAY_TESTS=1).
//! `HALO_BEVY_BRIDGE_NAME` names the bridge.

use halo_bridge::{Bridge, OBJECT_FIRST_PERSON, OBJECT_HIDDEN};
use std::time::{Duration, Instant};

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let walk = std::env::args().any(|a| a == "walk");
    // (the game hears from a point above the start for the second half)
    let listen = std::env::args().any(|a| a == "listen");
    // (a gamepad's left stick pushed forward for the second half)
    let pad = std::env::args().any(|a| a == "pad");
    // (the left mouse button held for the second half: firing)
    let fire = std::env::args().any(|a| a == "fire");
    // (for the second half: walking forward and strafing, throwing a
    // grenade (G) and meleeing (F) every two seconds, jumping and crouching
    // now and then)
    let grenades = std::env::args().any(|a| a == "grenades");
    // (a map file: the game's ray tests checked against halo_map's
    // collision code; the game needs HALO_BEVY_BRIDGE_RAY_TESTS=1)
    let map = std::env::args().find(|a| a.to_ascii_lowercase().ends_with(".map"));
    let mut collision = map.map(|path| CollisionCheck::new(&path));
    // (turns the view with the mouse every second and reports how long the
    // game's camera takes to follow)
    let latency = std::env::args().any(|a| a == "latency");
    let mut nudged: Option<(Instant, [f32; 3])> = None;
    let mut last_nudge = Instant::now();
    let mut samples: Vec<f64> = Vec::new();
    // (HALO_BEVY_BRIDGE_NAME, as the game takes it)
    let name = std::env::var("HALO_BEVY_BRIDGE_NAME").unwrap_or_else(|_| halo_bridge::DEFAULT_NAME.to_string());
    let mut input = halo_bridge::Input::default();
    let mut bridge = loop {
        match Bridge::open_named(&name) {
            Ok(bridge) => break bridge,
            Err(e) => {
                eprintln!("{e}; retrying");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    };
    let start = Instant::now();
    let report_latency = |samples: &[f64]| {
        if !samples.is_empty() {
            let mut sorted = samples.to_vec();
            sorted.sort_by(f64::total_cmp);
            println!(
                "mouse to the game's camera: {} samples, median {:.1} ms, min {:.1}, max {:.1}",
                sorted.len(),
                sorted[sorted.len() / 2],
                sorted[0],
                sorted[sorted.len() - 1]
            );
        }
    };
    let mut frames = 0u32;
    let mut last_report = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        if walk {
            let walking = start.elapsed() > Duration::from_secs(seconds / 2);
            input.active = u32::from(walking);
            input.set_key(26, walking); // W
            if walking {
                input.mouse_x += 1;
            }
            bridge.write_input(&input);
        }
        if listen {
            let listening = start.elapsed() > Duration::from_secs(seconds / 2);
            input.listener_valid = u32::from(listening);
            input.listener_position = [0.0, 0.0, 50.0];
            input.listener_forward = [1.0, 0.0, 0.0];
            input.listener_up = [0.0, 0.0, 1.0];
            bridge.write_input(&input);
        }
        if pad {
            let pushing = start.elapsed() > Duration::from_secs(seconds / 2);
            let mut gamepad = halo_bridge::Gamepad::default();
            if pushing {
                gamepad.axes[1] = -32767;
            }
            bridge.write_gamepads(&[gamepad]);
        }
        if fire {
            let firing = start.elapsed() > Duration::from_secs(seconds / 2);
            input.active = u32::from(firing);
            input.mouse_buttons = if firing { 1 << 1 } else { 0 };
            bridge.write_input(&input);
        }
        if grenades {
            let elapsed = start.elapsed();
            let active = elapsed > Duration::from_secs(seconds / 2);
            let tick = elapsed.as_millis() as u64;
            let (cycle, phase) = (tick / 2000, tick % 2000);
            input.active = u32::from(active);
            input.set_key(26, active); // W
            input.set_key(4, active && cycle % 3 == 1); // A
            input.set_key(7, active && cycle % 3 == 2); // D
            input.set_key(10, active && phase < 150); // G
            input.set_key(9, active && (1000..1100).contains(&phase)); // F (melee)
            input.set_key(44, active && cycle % 4 == 3 && phase < 100); // Space
            input.set_key(224, active && cycle % 5 == 4); // Left Ctrl
            if active {
                input.mouse_x += 1;
            }
            bridge.write_input(&input);
        }
        let fresh = bridge.read();
        if latency && start.elapsed() > Duration::from_secs(3) {
            if let (Some(snapshot), Some((at, forward))) = (&fresh, nudged)
                && let Some(camera) = snapshot.camera
                && camera.forward.iter().zip(forward).any(|(a, b)| (a - b).abs() > 1e-4)
            {
                samples.push(at.elapsed().as_secs_f64() * 1000.0);
                nudged = None;
            }
            if nudged.is_none() && last_nudge.elapsed() > Duration::from_millis(500)
                && let Some(camera) = fresh.as_ref().and_then(|s| s.camera)
            {
                input.active = 1;
                input.mouse_x += 40;
                bridge.write_input(&input);
                nudged = Some((Instant::now(), camera.forward));
                last_nudge = Instant::now();
            }
        }
        if let (Some(check), Some(snapshot)) = (collision.as_mut(), &fresh) {
            check.check(snapshot);
            let records = bridge.read_projectile_updates();
            if !records.is_empty() {
                let directions = bridge.random_directions();
                let tables = bridge.periodic_tables();
                check.check_projectiles(snapshot.structure_bsp_index, &records, &directions, tables.as_deref());
            }
            let bipeds = bridge.read_biped_updates();
            if !bipeds.is_empty() {
                check.check_bipeds(snapshot.structure_bsp_index, &bipeds);
            }
            let moving = bridge.read_biped_moving_records();
            if !moving.is_empty() {
                check.check_biped_moving(snapshot.structure_bsp_index, &moving);
            }
            let updates = bridge.read_biped_update_records();
            if !updates.is_empty() {
                check.check_biped_updates(snapshot.structure_bsp_index, &updates);
            }
            let animations = bridge.read_unit_animation_records();
            if !animations.is_empty() {
                check.check_unit_animations(&animations);
            }
            let poses = bridge.read_poses();
            if !poses.is_empty() {
                check.check_poses(&poses);
            }
            let vehicles = bridge.read_vehicle_physics();
            if !vehicles.is_empty() {
                check.check_vehicle_physics(snapshot.structure_bsp_index, &vehicles);
            }
            let vehicle_updates = bridge.read_vehicle_updates();
            if !vehicle_updates.is_empty() {
                check.check_vehicle_updates(snapshot.structure_bsp_index, &vehicle_updates);
            }
            let melees = bridge.read_melee_records();
            if !melees.is_empty() {
                check.check_melees(&melees);
            }
        }
        if let Some(snapshot) = fresh {
            frames += 1;
            if last_report.elapsed() >= Duration::from_secs(1) {
                let hidden = snapshot.objects.iter().filter(|o| o.flags & OBJECT_HIDDEN != 0).count();
                let first_person = snapshot.objects.iter().any(|o| o.flags & OBJECT_FIRST_PERSON != 0);
                let coloured: Vec<_> = snapshot
                    .objects
                    .iter()
                    .filter(|o| o.change_colors.iter().flatten().any(|&c| c < 0.999))
                    .collect();
                println!(
                    "{frames} frames/s | {} (map {}) bsp {} tick {} | {} objects ({hidden} hidden), {} nodes{} | camera {:?}",
                    snapshot.scenario_name,
                    snapshot.map_name(),
                    snapshot.structure_bsp_index,
                    snapshot.game_tick,
                    snapshot.objects.len(),
                    snapshot.nodes.len(),
                    if first_person { ", first person" } else { "" },
                    snapshot.camera.map(|c| c.position)
                );
                let camouflaged = snapshot.objects.iter().filter(|o| o.active_camouflage > 0.0).count();
                if camouflaged > 0 {
                    println!("  {camouflaged} camouflaged objects");
                }
                if let Some(light) = snapshot.lights.first() {
                    println!("  {} dynamic lights, e.g. {light:?}", snapshot.lights.len());
                }
                if let Some(object) = coloured.first() {
                    println!(
                        "  {} objects with change colours, e.g. {:08x}: {:?}",
                        coloured.len(),
                        object.object_index,
                        object.change_colors
                    );
                }
                frames = 0;
                last_report = Instant::now();
            }
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    report_latency(&samples);
    if let Some(check) = &collision {
        check.report();
    }
}

/// `normalize3d` then `scale_vector3d(v, scale)`, as the game does them.
fn normalize_and_scale(v: [f32; 3], length: f32, scale: f32) -> [f32; 3] {
    let normalized = if length.abs() < 0.0001 {
        v
    } else {
        let inverse = 1.0 / length;
        [v[0] * inverse, v[1] * inverse, v[2] * inverse]
    };
    [normalized[0] * scale, normalized[1] * scale, normalized[2] * scale]
}

/// The game's ray tests against halo_map's collision code: how many agree
/// (hit or miss, the surface, and t to within a millionth).
struct CollisionCheck {
    cache: halo_map::CacheFile,
    bsps: std::collections::HashMap<i32, Option<halo_map::collision::CollisionBsp>>,
    rays: u64,
    agreed: u64,
    spheres: u64,
    spheres_agreed: u64,
    pills: u64,
    pills_agreed: u64,
    pill_hits: u64,
    object_hits: u64,
    object_hits_agreed: u64,
    whole_tests: u64,
    whole_agreed: u64,
    points: u64,
    points_agreed: u64,
    point_collisions: u64,
    points_in_water: u64,
    point_definition_flags: u32,
    point_definitions: std::collections::HashMap<u32, Option<halo_map::physics::PointPhysicsDefinition>>,
    clusters: std::collections::HashMap<i32, Option<halo_map::collision::StructureClusters>>,
    models: std::collections::HashMap<u32, Option<std::sync::Arc<halo_map::collision::CollisionModel>>>,
    sphere_features: u64,
    projectile_updates: u64,
    projectiles_replayed: u64,
    projectiles_agreed: u64,
    projectile_collisions: u64,
    projectile_skipped: [u64; 4],
    projectile_definitions: std::collections::HashMap<u32, Option<halo_map::projectile::ProjectileDefinition>>,
    projectiles_seen: std::collections::HashSet<u32>,
    projectile_actions: [u64; 3],
    projectiles_shown: u32,
    biped_updates: u64,
    bipeds_replayed: u64,
    bipeds_agreed: u64,
    bipeds_touched_objects: u64,
    bipeds_incomplete: u64,
    biped_object_features: u64,
    /// Agreeing updates by the type of object bumped (0 biped ... 8
    /// control), and those riding an elevator.
    bipeds_bumped_kinds: [u64; 12],
    bipeds_on_elevators: u64,
    bipeds_elsewhere: u64,
    bipeds_seen: std::collections::HashSet<u32>,
    biped_kinds: [u64; 6],
    bipeds_shown: u32,
    /// Recent biped physics updates by sequence, with the objects their
    /// features came from, for the movement updates that made them.
    recent_physics: std::collections::HashMap<u32, (halo_bridge::BipedRecord, Vec<Vec<halo_map::features::FeatureObject>>)>,
    biped_definitions: std::collections::HashMap<u32, Option<halo_map::biped_moving::BipedDefinition>>,
    player_information: Option<Option<halo_map::biped_moving::PlayerInformation>>,
    moving_updates: u64,
    moving_replayed: u64,
    moving_agreed: u64,
    moving_physics_compared: u64,
    moving_physics_agreed: u64,
    moving_missing_physics: u64,
    moving_elsewhere: u64,
    /// Of the agreeing updates: player physics, animation-driven, flying,
    /// turned by the animation, jumps, landings started, aborted animations,
    /// crouch changes.
    moving_kinds: [u64; 8],
    moving_shown: u32,
    /// Recent movement updates by sequence: what they read, and their
    /// physics' world.
    recent_moving: std::collections::HashMap<
        u32,
        (halo_map::biped_moving::MovingInputs, Option<(halo_bridge::BipedRecord, Vec<Vec<halo_map::features::FeatureObject>>)>),
    >,
    update_updates: u64,
    update_replayed: u64,
    update_agreed: u64,
    update_missing: u64,
    update_elsewhere: u64,
    /// Of the agreeing updates: dead, airborne, landing, flying turns,
    /// moving turns, standing turns asked for, tumbling.
    update_kinds: [u64; 7],
    update_shown: u32,
    animation_graphs: std::collections::HashMap<u32, Option<halo_map::unit_animation::AnimationGraph>>,
    animation_updates: u64,
    animation_agreed: u64,
    /// Of the agreeing updates: states entered, seat or weapon changes,
    /// animations restarted on a new permutation (the seed moved), side
    /// effects, leaps.
    animation_kinds: [u64; 5],
    animation_side_effects: u32,
    animation_shown: u32,
    pose_definitions: std::collections::HashMap<u32, Option<halo_map::pose::PoseObjectDefinition>>,
    pose_models: std::collections::HashMap<u32, Option<halo_map::pose::PoseModel>>,
    pose_animations: std::collections::HashMap<(u32, i16), Option<halo_map::pose::PoseAnimation>>,
    poses: u64,
    poses_incomplete: u64,
    /// Orientations from the animation: compared, agreed, left out for
    /// overlays; and of the agreeing, from compressed animations, from the
    /// model's pose.
    pose_animated: [u64; 5],
    /// Matrices: compared, agreed, of those interpolating, with a parent.
    pose_matrices: [u64; 4],
    pose_types: [u64; 12],
    pose_shown: u32,
    /// Of the agreeing projectile updates: touching an object, damaging
    /// one, sticking to one, overpenetrating one, steering, super-combining.
    projectile_object_kinds: [u64; 6],
    /// Rays at vehicles tested with their physics shapes: compared, agreed;
    /// of the agreeing, hitting a vehicle, another object, the structure.
    vehicle_rays: [u64; 5],
    physics_shapes: std::collections::HashMap<u32, Option<std::sync::Arc<halo_map::vehicle_physics::PhysicsShape>>>,
    /// Bipeds' preprocess: compared, agreed; of the agreeing, with an
    /// action, an overlay action, a soft ping, an emotion, mouth or
    /// acceleration overlay, aiming, looking, throwing a grenade (enhanced).
    unit_poses: [u64; 9],
    /// Movement and update records whose biped the game erased for falling
    /// outside the world (not ported).
    moving_erased: u64,
    update_erased: u64,
    unit_poses_shown: u32,
    /// Of the agreeing grenade throws: moving, airborne, crouched, riding,
    /// easing their legs' weights.
    unit_pose_throws: [u64; 5],
    /// Each type's preprocess (biped, vehicle, weapon, device, other):
    /// compared, agreed, of those changing the pose.
    preprocess_kinds: [[u64; 3]; 5],
    /// Players' melee steps: compared, agreed; of the agreeing, started
    /// (with a weapon, without), damage landed, under way.
    melees: [u64; 6],
    /// Vehicles' physics updates: recorded, compared, agreed (state and
    /// mass points), state alone agreed; left out: old physics, rings
    /// lost, elsewhere; of the agreeing: driven, moved, frozen against the
    /// world, on the ground, in water, colliding with another vehicle,
    /// antigraving.
    vehicle_physics: [u64; 14],
    vehicle_physics_shown: u32,
    recent_vehicle_physics: std::collections::HashMap<u32, halo_bridge::VehiclePhysicsUpdate>,
    /// Vehicle updates: recorded, compared, agreed; left out elsewhere,
    /// missing their physics record; controls agreed; of the agreeing:
    /// with physics, coasting, riding a parent, righting, with a throttle.
    vehicle_updates: [u64; 11],
    vehicle_update_types: [u64; 7],
    vehicle_updates_shown: u32,
    vehicle_definitions: std::collections::HashMap<u32, Option<std::sync::Arc<halo_map::vehicle_update::VehicleDefinition>>>,
    vehicle_bounds: std::collections::HashMap<i32, Option<(f32, f32)>>,
    physics_definitions: std::collections::HashMap<u32, Option<std::sync::Arc<halo_map::vehicle_physics::PhysicsDefinition>>>,
    material_physics: Option<Vec<halo_map::vehicle_physics::MaterialPhysics>>,
    melees_shown: u32,
    melee_weapons: std::collections::HashMap<u32, Option<halo_map::biped_update::MeleeWeapon>>,
    /// Of the agreeing vehicles: turning, with a speed, a wheel turned,
    /// with a suspension compressed; devices: with a position, a power.
    vehicle_poses: [u64; 4],
    device_poses: [u64; 2],
    vehicle_pose_definitions: std::collections::HashMap<u32, Option<halo_map::vehicle_pose::VehiclePoseDefinition>>,
    device_pose_definitions: std::collections::HashMap<u32, Option<halo_map::device_pose::DevicePoseDefinition>>,
    /// The graphs whose animations are loaded for posing.
    unit_pose_graphs: std::collections::HashSet<u32>,
    frames_shown: u32,
    hits: u64,
    largest_t_difference: f32,
    shown: [u32; 5],
}

impl CollisionCheck {
    fn new(path: &str) -> Self {
        let cache = halo_map::CacheFile::open(std::path::Path::new(path)).expect("the map opens");
        CollisionCheck {
            cache,
            bsps: Default::default(),
            rays: 0,
            agreed: 0,
            spheres: 0,
            spheres_agreed: 0,
            pills: 0,
            pills_agreed: 0,
            pill_hits: 0,
            object_hits: 0,
            object_hits_agreed: 0,
            whole_tests: 0,
            whole_agreed: 0,
            points: 0,
            points_agreed: 0,
            point_collisions: 0,
            points_in_water: 0,
            point_definition_flags: 0,
            point_definitions: Default::default(),
            clusters: Default::default(),
            models: Default::default(),
            projectile_updates: 0,
            projectiles_replayed: 0,
            projectiles_agreed: 0,
            projectile_collisions: 0,
            projectile_skipped: [0; 4],
            projectile_definitions: Default::default(),
            projectiles_seen: Default::default(),
            projectile_actions: [0; 3],
            projectiles_shown: 0,
            biped_updates: 0,
            bipeds_replayed: 0,
            bipeds_agreed: 0,
            bipeds_touched_objects: 0,
            bipeds_incomplete: 0,
            biped_object_features: 0,
            bipeds_bumped_kinds: [0; 12],
            bipeds_on_elevators: 0,
            bipeds_elsewhere: 0,
            bipeds_seen: Default::default(),
            biped_kinds: [0; 6],
            bipeds_shown: 0,
            recent_physics: Default::default(),
            biped_definitions: Default::default(),
            player_information: None,
            moving_updates: 0,
            moving_replayed: 0,
            moving_agreed: 0,
            moving_physics_compared: 0,
            moving_physics_agreed: 0,
            moving_missing_physics: 0,
            moving_elsewhere: 0,
            moving_kinds: [0; 8],
            moving_shown: 0,
            recent_moving: Default::default(),
            update_updates: 0,
            update_replayed: 0,
            update_agreed: 0,
            update_missing: 0,
            update_elsewhere: 0,
            update_kinds: [0; 7],
            update_shown: 0,
            animation_graphs: Default::default(),
            animation_updates: 0,
            animation_agreed: 0,
            animation_kinds: [0; 5],
            animation_side_effects: 0,
            animation_shown: 0,
            pose_definitions: Default::default(),
            pose_models: Default::default(),
            pose_animations: Default::default(),
            poses: 0,
            poses_incomplete: 0,
            pose_animated: [0; 5],
            pose_matrices: [0; 4],
            pose_types: [0; 12],
            pose_shown: 0,
            projectile_object_kinds: [0; 6],
            vehicle_rays: [0; 5],
            physics_shapes: Default::default(),
            unit_poses: [0; 9],
            moving_erased: 0,
            update_erased: 0,
            unit_poses_shown: 0,
            unit_pose_throws: [0; 5],
            preprocess_kinds: [[0; 3]; 5],
            melees: [0; 6],
            vehicle_physics: [0; 14],
            vehicle_physics_shown: 0,
            recent_vehicle_physics: Default::default(),
            vehicle_updates: [0; 11],
            vehicle_update_types: [0; 7],
            vehicle_updates_shown: 0,
            vehicle_definitions: Default::default(),
            vehicle_bounds: Default::default(),
            physics_definitions: Default::default(),
            material_physics: None,
            melees_shown: 0,
            melee_weapons: Default::default(),
            vehicle_poses: [0; 4],
            device_poses: [0; 2],
            vehicle_pose_definitions: Default::default(),
            device_pose_definitions: Default::default(),
            unit_pose_graphs: Default::default(),
            frames_shown: 0,
            sphere_features: 0,
            hits: 0,
            largest_t_difference: 0.0,
            shown: [0; 5],
        }
    }

    fn check(&mut self, snapshot: &halo_bridge::Snapshot) {
        let before = self.rays + self.spheres + self.pills + self.whole_tests + self.points
            - (self.agreed + self.spheres_agreed + self.pills_agreed + self.whole_agreed + self.points_agreed);
        self.check_frame(snapshot);
        let after = self.rays + self.spheres + self.pills + self.whole_tests + self.points
            - (self.agreed + self.spheres_agreed + self.pills_agreed + self.whole_agreed + self.points_agreed);
        if after != before && self.frames_shown < 40 {
            self.frames_shown += 1;
            println!(
                "  frame {} tick {} bsp {} camera {:?}: {} differ",
                snapshot.frame_index,
                snapshot.game_tick,
                snapshot.structure_bsp_index,
                snapshot.camera.map(|c| c.position),
                after - before
            );
        }
    }

    fn check_frame(&mut self, snapshot: &halo_bridge::Snapshot) {
        if snapshot.ray_tests.is_empty() {
            return;
        }
        let cache = &self.cache;
        let bsp = self
            .bsps
            .entry(snapshot.structure_bsp_index)
            .or_insert_with(|| {
                let reference = halo_map::bsp::references(cache)
                    .ok()?
                    .into_iter()
                    .find(|r| r.index as i32 == snapshot.structure_bsp_index)?;
                let loaded = halo_map::bsp::Bsp::load(cache, &reference).ok()?;
                halo_map::collision::CollisionBsp::load(&loaded).ok()
            })
            .as_ref();
        let Some(bsp) = bsp else { return };
        let clusters = self
            .clusters
            .entry(snapshot.structure_bsp_index)
            .or_insert_with(|| {
                let reference = halo_map::bsp::references(cache)
                    .ok()?
                    .into_iter()
                    .find(|r| r.index as i32 == snapshot.structure_bsp_index)?;
                let loaded = halo_map::bsp::Bsp::load(cache, &reference).ok()?;
                halo_map::collision::StructureClusters::load(cache, &loaded).ok()
            })
            .as_ref();
        let Some(clusters) = clusters else { return };
        // the objects as the broad phase sees them
        let mut world_objects = Vec::new();
        for object in &snapshot.objects {
            if object.flags & halo_bridge::OBJECT_MODEL != 0 {
                continue;
            }
            let definition = halo_map::TagRef { group: 0, datum: Some(object.definition_index) };
            let model = self
                .models
                .entry(object.definition_index)
                .or_insert_with(|| {
                    let datum = halo_map::objects::object_collision_model(cache, definition).ok()??.datum?;
                    halo_map::collision::CollisionModel::load(cache, datum).ok().map(std::sync::Arc::new)
                })
                .clone();
            // (a vehicle's physics shape, placed as the game recorded it)
            let physics = match snapshot.physics_vehicles.iter().find(|v| v.object_index == object.object_index) {
                Some(vehicle) if object.kind == 1 => self
                    .physics_shapes
                    .entry(object.definition_index)
                    .or_insert_with(|| {
                        halo_map::vehicle_physics::PhysicsShape::load_for_object(cache, object.definition_index).ok().flatten().map(std::sync::Arc::new)
                    })
                    .clone()
                    .map(|shape| halo_map::vehicle_physics::physics_instance_new(shape, vehicle.origin, vehicle.forward, vehicle.up)),
                _ => None,
            };
            world_objects.push(halo_map::collision::WorldObject {
                index: object.object_index,
                kind: object.kind,
                flags: object.object_flags,
                bounding_center: object.bounding_center,
                bounding_radius: object.bounding_radius,
                parent: object.parent_index,
                location_cluster: object.location_cluster,
                region_permutations: object.region_permutations,
                matrices: snapshot
                    .object_nodes(object)
                    .iter()
                    .map(|n| halo_map::model::Matrix4x3 { scale: n.scale, forward: n.forward, left: n.left, up: n.up, position: n.position })
                    .collect(),
                model,
                physics,
            });
        }
        // rays at vehicles, tested with their physics shapes
        for test in &snapshot.vehicle_ray_tests {
            use halo_map::collision::{CollisionKind, flags, test_flags};
            let world = halo_map::collision::World { collision: bsp, clusters, objects: &world_objects, source: None };
            let ours = world.test(
                flags::FRONT_FACING
                    | test_flags::STRUCTURE
                    | test_flags::OBJECTS
                    | test_flags::ALL_OBJECT_TYPES
                    | test_flags::USE_VEHICLE_PHYSICS,
                test.point,
                test.vector,
                u32::MAX,
            );
            let kind = match ours.kind {
                None => -1,
                Some(CollisionKind::Media) => 0,
                Some(CollisionKind::Structure) => 2,
                Some(CollisionKind::Object) => 3,
            };
            let bits = |p: [f32; 4]| p.map(f32::to_bits);
            let agrees = kind == test.kind
                && ours.t.to_bits() == test.t.to_bits()
                && (kind == -1 || bits(ours.plane) == bits(test.plane))
                && ours.object.unwrap_or(u32::MAX) == test.object_index
                // (a structure hit leaves the node as it was)
                && (kind == -1 || ours.surface == test.surface_index)
                && (kind != 3 || ours.node == test.node_index);
            self.vehicle_rays[0] += 1;
            if agrees {
                self.vehicle_rays[1] += 1;
                let vehicle = test.kind == 3 && snapshot.objects.iter().any(|o| o.object_index == test.object_index && o.kind == 1);
                self.vehicle_rays[2] += vehicle as u64;
                self.vehicle_rays[3] += (test.kind == 3 && !vehicle) as u64;
                self.vehicle_rays[4] += (test.kind == 2) as u64;
            } else if self.shown[0] < 10 {
                self.shown[0] += 1;
                println!(
                    "  vehicle ray differs: game type {} object {:08x} node {} t {} plane {:?} surface {}; halo_map {:?}",
                    test.kind, test.object_index, test.node_index, test.t, test.plane, test.surface_index, ours
                );
            }
        }
        for test in &snapshot.ray_tests {
            let ours = bsp.test_vector(halo_map::collision::flags::FRONT_FACING, test.point, test.vector, 1.0);
            self.rays += 1;
            let agree = match ours {
                Some(hit) => {
                    let difference = (hit.t - test.t).abs();
                    self.largest_t_difference = self.largest_t_difference.max(difference);
                    test.hit != 0 && hit.surface as i32 == test.surface_index && difference <= 1e-6
                }
                None => test.hit == 0,
            };
            if test.hit != 0 {
                self.hits += 1;
            }
            // the game's whole ray test: structure and objects
            {
                let world_objects = &world_objects;
                let world = halo_map::collision::World { collision: bsp, clusters, objects: world_objects, source: None };
                let flags = halo_map::collision::flags::FRONT_FACING
                    | halo_map::collision::test_flags::STRUCTURE
                    | halo_map::collision::test_flags::OBJECTS
                    | halo_map::collision::test_flags::ALL_OBJECT_TYPES;
                let ours = world.test_vector(flags, test.point, test.vector, u32::MAX);
                if test.object_index != u32::MAX {
                    self.object_hits += 1;
                }
                let agrees = match ours {
                    Some(hit) => {
                        hit.t.to_bits() == test.object_t.to_bits()
                            && hit.object.unwrap_or(u32::MAX) == test.object_index
                            && hit.surface == test.object_surface_index
                            && (hit.object.is_none()
                                || (hit.node == test.object_node_index && hit.bsp == test.object_bsp_index))
                    }
                    None => test.object_surface_index == -1 && test.object_index == u32::MAX,
                };
                self.whole_tests += 1;
                if agrees {
                    self.whole_agreed += 1;
                    if test.object_index != u32::MAX {
                        self.object_hits_agreed += 1;
                    }
                } else if self.shown[0] < 10 {
                    self.shown[0] += 1;
                    println!(
                        "  whole test differs: game object {:08x} node {} bsp {} t {} surface {}; halo_map {:?}",
                        test.object_index, test.object_node_index, test.object_bsp_index, test.object_t, test.object_surface_index, ours
                    );
                }
            }
            // the point physics
            let definition = *self.point_definitions.entry(snapshot.point_physics_definition).or_insert_with(|| {
                (snapshot.point_physics_definition != u32::MAX)
                    .then(|| halo_map::physics::PointPhysicsDefinition::load(cache, snapshot.point_physics_definition).ok())
                    .flatten()
            });
            if let Some(definition) = definition {
                let world = halo_map::collision::World { collision: bsp, clusters, objects: &world_objects, source: None };
                let length = (test.vector[0] * test.vector[0] + test.vector[1] * test.vector[1] + test.vector[2] * test.vector[2]).sqrt();
                let mut state = halo_map::physics::PointState {
                    position: test.point,
                    velocity: normalize_and_scale(test.vector, length, 15.0),
                    location_leaf: -1,
                    location_cluster: -1,
                };
                let mut result_flags = 0;
                let mut collisions = 0;
                for _ in 0..halo_bridge::POINT_PHYSICS_STEPS {
                    let (result, _) = halo_map::physics::point_physics_update(
                        &world,
                        0,
                        &definition,
                        &mut state,
                        [0.0; 3],
                        test.point_underwater != 0,
                        None,
                        test.point_radius,
                        1.0 / 30.0,
                    );
                    result_flags |= result;
                    if result & (halo_map::physics::result_flags::COLLIDED_WITH_STRUCTURE | halo_map::physics::result_flags::COLLIDED_WITH_WATER) != 0 {
                        collisions += 1;
                    }
                }
                self.points += 1;
                self.point_collisions += collisions as u64;
                self.point_definition_flags = definition.flags;
                if test.point_result_flags & halo_map::physics::result_flags::COLLIDED_WITH_WATER != 0 {
                    self.points_in_water += 1;
                }
                if state.position.map(f32::to_bits) == test.point_position.map(f32::to_bits)
                    && state.velocity.map(f32::to_bits) == test.point_velocity.map(f32::to_bits)
                    && result_flags == test.point_result_flags
                    && collisions == test.point_collisions
                {
                    self.points_agreed += 1;
                } else if self.shown[1] < 10 {
                    self.shown[1] += 1;
                    println!(
                        "  point differs: game {:?} {:?} flags {} collisions {}; halo_map {:?} {:?} flags {result_flags} collisions {collisions}",
                        test.point_position, test.point_velocity, test.point_result_flags, test.point_collisions, state.position, state.velocity
                    );
                }
            }
            // the swept sphere
            let pill = bsp.test_pill(test.point, test.pill_vector, test.pill_radius, f32::MAX);
            self.pills += 1;
            if test.pill_hit != 0 {
                self.pill_hits += 1;
            }
            let pill_agrees = match pill {
                Some(hit) => {
                    test.pill_hit != 0
                        && hit.t.to_bits() == test.pill_t.to_bits()
                        && hit.surface == test.pill_surface_index
                        && [hit.plane[0], hit.plane[1], hit.plane[2]].map(f32::to_bits) == test.pill_normal.map(f32::to_bits)
                }
                None => test.pill_hit == 0,
            };
            if pill_agrees {
                self.pills_agreed += 1;
            } else if self.shown[2] < 10 {
                self.shown[2] += 1;
                println!(
                    "  swept sphere differs (radius {}): game hit {} t {} surface {} normal {:?}; halo_map {:?}",
                    test.pill_radius, test.pill_hit, test.pill_t, test.pill_surface_index, test.pill_normal, pill
                );
            }
            // the sphere
            let features = bsp.test_sphere(test.sphere_center, test.sphere_radius);
            let mut checksum = 2_166_136_261u32;
            for list in [&features.surfaces, &features.edges, &features.vertices, &features.leaves] {
                checksum = halo_bridge::feature_checksum(checksum, list);
            }
            let counts = [features.surfaces.len(), features.edges.len(), features.vertices.len(), features.leaves.len()]
                .map(|c| c as u16);
            self.spheres += 1;
            self.sphere_features += (features.surfaces.len() + features.edges.len() + features.vertices.len()) as u64;
            if counts == test.sphere_counts && checksum == test.sphere_checksum {
                self.spheres_agreed += 1;
            } else if self.shown[3] < 10 {
                self.shown[3] += 1;
                println!(
                    "  sphere differs (radius {}): game counts {:?}; halo_map {:?}",
                    test.sphere_radius, test.sphere_counts, counts
                );
            }
            if agree {
                self.agreed += 1;
            } else if self.shown[4] < 5 {
                self.shown[4] += 1;
                println!(
                    "  ray differs: game hit {} t {} surface {}; halo_map {:?}",
                    test.hit, test.t, test.surface_index, ours.map(|h| (h.t, h.surface))
                );
            }
        }
    }

    /// Replays the projectile updates the game recorded with halo_map's
    /// `projectile_update`, from the state each began with and the random
    /// seeds the game had, and compares the states they leave bit for bit.
    /// Updates that hit an object and guided ones are left out (halo_map
    /// doesn't port them), as are ones with more collisions than the game
    /// records seeds for.
    fn check_projectiles(
        &mut self,
        structure_bsp_index: i32,
        updates: &[halo_bridge::ProjectileUpdate],
        directions: &[[f32; 3]],
        periodic_tables: Option<&[[u8; 1024]; 12]>,
    ) {
        let cache = &self.cache;
        fn load(cache: &halo_map::CacheFile, structure_bsp_index: i32) -> Option<halo_map::bsp::Bsp<'_>> {
            let reference =
                halo_map::bsp::references(cache).ok()?.into_iter().find(|r| r.index as i32 == structure_bsp_index)?;
            halo_map::bsp::Bsp::load(cache, &reference).ok()
        }
        if !self.bsps.contains_key(&structure_bsp_index) {
            let loaded = load(cache, structure_bsp_index).and_then(|b| halo_map::collision::CollisionBsp::load(&b).ok());
            self.bsps.insert(structure_bsp_index, loaded);
        }
        if !self.clusters.contains_key(&structure_bsp_index) {
            let loaded = load(cache, structure_bsp_index).and_then(|b| halo_map::collision::StructureClusters::load(cache, &b).ok());
            self.clusters.insert(structure_bsp_index, loaded);
        }
        let (Some(Some(bsp)), Some(Some(clusters))) = (self.bsps.get(&structure_bsp_index), self.clusters.get(&structure_bsp_index)) else {
            return;
        };
        for update in updates {
            let record = &update.record;
            self.projectile_updates += 1;
            self.projectiles_seen.insert(record.projectile_index);
            let skip = if record.structure_bsp_index != structure_bsp_index {
                Some(0)
            } else if !update.complete {
                Some(1)
            } else if record.collision_count as usize > record.collision_seeds.len() {
                Some(3)
            } else {
                None
            };
            if let Some(reason) = skip {
                self.projectile_skipped[reason] += 1;
                continue;
            }
            let definition = self
                .projectile_definitions
                .entry(record.definition_index)
                .or_insert_with(|| halo_map::projectile::ProjectileDefinition::load(cache, record.definition_index).ok());
            let Some(definition) = definition.as_ref() else {
                self.projectile_skipped[0] += 1;
                continue;
            };
            let b = &record.before;
            let mut state = halo_map::projectile::ProjectileState {
                position: b.position,
                velocity: b.velocity,
                flags: b.projectile_flags,
                object_flags: b.object_flags,
                action: b.action,
                hit_material_type: b.hit_material_type,
                ignore_object_index: b.ignore_object_index,
                target_object_index: b.target_object_index,
                parent_object_index: b.parent_object_index,
                detonation_timer: b.detonation_timer,
                detonation_timer_delta: b.detonation_timer_delta,
                arming_time: b.arming_time,
                arming_time_delta: b.arming_time_delta,
                odometer: b.odometer,
                deceleration_timer: b.deceleration_timer,
                deceleration_timer_delta: b.deceleration_timer_delta,
                deceleration: b.deceleration,
                maximum_damage_distance: b.maximum_damage_distance,
                location_leaf: b.location_leaf,
                location_cluster: b.location_cluster,
            };
            // the objects its ray tests visited, as they were then (each once,
            // as first seen)
            let mut seen = std::collections::HashSet::new();
            let mut world_objects = Vec::new();
            let mut objects = Vec::new();
            for (entry, matrices) in &update.objects {
                if !seen.insert(entry.object_index) {
                    continue;
                }
                let definition = halo_map::TagRef { group: 0, datum: Some(entry.definition_index) };
                let model = self
                    .models
                    .entry(entry.definition_index)
                    .or_insert_with(|| {
                        let datum = halo_map::objects::object_collision_model(cache, definition).ok()??.datum?;
                        halo_map::collision::CollisionModel::load(cache, datum).ok().map(std::sync::Arc::new)
                    })
                    .clone();
                world_objects.push(halo_map::collision::WorldObject {
                    index: entry.object_index,
                    kind: entry.kind,
                    flags: entry.flags,
                    bounding_center: entry.bounding_center,
                    bounding_radius: entry.bounding_radius,
                    parent: entry.parent_object_index as u32,
                    location_cluster: entry.location_cluster,
                    region_permutations: entry.region_permutations,
                    matrices: matrices
                        .iter()
                        .map(|m| halo_map::model::Matrix4x3 {
                            scale: m[0],
                            forward: [m[1], m[2], m[3]],
                            left: [m[4], m[5], m[6]],
                            up: [m[7], m[8], m[9]],
                            position: [m[10], m[11], m[12]],
                        })
                        .collect(),
                    model,
                    physics: None,
                });
                objects.push(halo_map::projectile::ProjectileObject {
                    index: entry.object_index,
                    definition_index: entry.definition_index,
                    kind: entry.kind,
                    parent: entry.parent_object_index as u32,
                    projectile_flags: entry.projectile_flags,
                });
            }
            let world = halo_map::collision::World { collision: bsp, clusters, objects: &world_objects, source: None };
            let guidance: Vec<halo_map::projectile::Guidance> = record.guidance[..(record.guidance_count as usize).min(4)]
                .iter()
                .map(|g| halo_map::projectile::Guidance {
                    projectile_center: g.projectile_center,
                    target_center: g.target_center,
                    target_center_of_mass: g.target_center_of_mass,
                    guidance_scale: g.guidance_scale,
                    game_time: g.game_time,
                })
                .collect();
            let mut context = halo_map::projectile::ProjectileContext {
                projectile_index: record.projectile_index,
                definition_index: record.definition_index,
                collision_seeds: &record.collision_seeds[..record.collision_count as usize],
                damage_material_types: &record.damage_material_types[..(record.collision_count as usize).min(4)],
                damage_seeds: &record.damage_seeds[..(record.collision_count as usize).min(4)],
                objects: &objects,
                guidance: &guidance,
                // (_periodic_function_wander)
                wander_table: periodic_tables.map(|t| &t[10]),
            };
            let mut random = halo_map::projectile::Random { seed: record.random_seed, directions };
            let summary = match halo_map::projectile::projectile_update(&world, definition, &mut state, &mut random, &mut context) {
                Ok(summary) => summary,
                Err(missing) => {
                    self.projectile_skipped[2] += 1;
                    if self.projectiles_shown < 10 {
                        self.projectiles_shown += 1;
                        println!("  projectile {:#x} not replayed: {missing:?} missing", record.projectile_index);
                    }
                    continue;
                }
            };
            self.projectiles_replayed += 1;
            self.projectile_collisions += record.collision_count as u64;
            let a = &record.after;
            let bits3 = |v: [f32; 3]| v.map(f32::to_bits);
            let same = bits3(state.position) == bits3(a.position)
                && bits3(state.velocity) == bits3(a.velocity)
                && state.flags == a.projectile_flags
                && state.object_flags == a.object_flags
                && state.action == a.action
                && state.hit_material_type == a.hit_material_type
                && state.ignore_object_index == a.ignore_object_index
                && state.detonation_timer.to_bits() == a.detonation_timer.to_bits()
                && state.detonation_timer_delta.to_bits() == a.detonation_timer_delta.to_bits()
                && state.arming_time.to_bits() == a.arming_time.to_bits()
                && state.odometer.to_bits() == a.odometer.to_bits()
                && state.deceleration_timer.to_bits() == a.deceleration_timer.to_bits()
                && state.deceleration_timer_delta.to_bits() == a.deceleration_timer_delta.to_bits()
                && state.deceleration.to_bits() == a.deceleration.to_bits()
                && state.maximum_damage_distance.to_bits() == a.maximum_damage_distance.to_bits()
                && state.location_leaf == a.location_leaf
                && state.location_cluster == a.location_cluster
                && summary.collisions == record.collision_count as usize;
            if same {
                self.projectiles_agreed += 1;
                if let Some(count) = self.projectile_actions.get_mut(a.action as usize) {
                    *count += 1;
                }
                let k = &mut self.projectile_object_kinds;
                k[0] += (record.touched_object != 0) as u64;
                k[1] += summary.damaged_objects as u64;
                k[2] += summary.attached_to.is_some() as u64;
                k[3] += (a.ignore_object_index != -1) as u64;
                k[4] += (summary.steered > 0) as u64;
                k[5] += (state.flags & halo_map::projectile::flags::WILL_SUPER_EXPLODE != 0) as u64;
            } else if self.projectiles_shown < 10 {
                self.projectiles_shown += 1;
                println!(
                    "  projectile {:#x} (definition {:#x}) differs after {} collisions (halo_map {}):\n    before {:?}\n    game   {:?}\n    halo_map {:?}\n    summary {:?}; touched {}; damage {:?}; guidance {:?}",
                    record.projectile_index,
                    record.definition_index,
                    record.collision_count,
                    summary.collisions,
                    record.before,
                    record.after,
                    state,
                    summary,
                    record.touched_object,
                    record.damage_material_types,
                    &record.guidance[..(record.guidance_count as usize).min(4)]
                );
            }
        }
    }

    /// Replays the biped physics updates the game recorded with halo_map's
    /// `biped_update_physics`, from the physics each began with and the
    /// breakable surfaces standing then, with the objects the game's broad
    /// phase visited as they were then, and compares what they leave byte
    /// for byte. `biped_kinds[3]`-style counts are of agreeing updates;
    /// `bipeds_touched_objects` counts those that bumped an object.
    fn check_bipeds(&mut self, structure_bsp_index: i32, updates: &[halo_bridge::BipedUpdate]) {
        use halo_map::biped::{in_flags, out_flags};
        let cache = &self.cache;
        fn load(cache: &halo_map::CacheFile, structure_bsp_index: i32) -> Option<halo_map::bsp::Bsp<'_>> {
            let reference =
                halo_map::bsp::references(cache).ok()?.into_iter().find(|r| r.index as i32 == structure_bsp_index)?;
            halo_map::bsp::Bsp::load(cache, &reference).ok()
        }
        if !self.bsps.contains_key(&structure_bsp_index) {
            let loaded = load(cache, structure_bsp_index).and_then(|b| halo_map::collision::CollisionBsp::load(&b).ok());
            self.bsps.insert(structure_bsp_index, loaded);
        }
        if !matches!(self.bsps.get(&structure_bsp_index), Some(Some(_))) {
            return;
        }
        for update in updates {
            let record = &update.record;
            self.biped_updates += 1;
            self.bipeds_seen.insert(record.biped_index);
            if record.structure_bsp_index != structure_bsp_index {
                self.bipeds_elsewhere += 1;
                continue;
            }
            if !update.complete {
                self.bipeds_incomplete += 1;
                continue;
            }
            // the objects visited, per gathering of features
            let mut objects: Vec<Vec<halo_map::features::FeatureObject>> = vec![Vec::new(); record.feature_calls as usize];
            for (entry, matrices) in &update.objects {
                let definition = halo_map::TagRef { group: 0, datum: Some(entry.definition_index) };
                let model = self
                    .models
                    .entry(entry.definition_index)
                    .or_insert_with(|| {
                        let datum = halo_map::objects::object_collision_model(cache, definition).ok()??.datum?;
                        halo_map::collision::CollisionModel::load(cache, datum).ok().map(std::sync::Arc::new)
                    })
                    .clone();
                let object = halo_map::features::FeatureObject {
                    index: entry.object_index as i32,
                    kind: entry.kind,
                    flags: entry.flags,
                    dead: entry.dead != 0,
                    bounding_center: entry.bounding_center,
                    bounding_radius: entry.bounding_radius,
                    parent: entry.parent_object_index,
                    velocity: entry.translational_velocity,
                    elevator: entry.elevator != 0,
                    biped: (entry.kind == halo_map::features::OBJECT_TYPE_BIPED).then_some(halo_map::features::BipedPill {
                        passes_through_bipeds: entry.biped_passes_through != 0,
                        player: entry.player_index != -1,
                        seated: entry.parent_seat_index != -1,
                        base: entry.pill_base,
                        height: entry.pill_height,
                        width: entry.pill_width,
                    }),
                    model,
                    region_permutations: entry.region_permutations,
                    matrices: matrices
                        .iter()
                        .map(|m| halo_map::model::Matrix4x3 {
                            scale: m[0],
                            forward: [m[1], m[2], m[3]],
                            left: [m[4], m[5], m[6]],
                            up: [m[7], m[8], m[9]],
                            position: [m[10], m[11], m[12]],
                        })
                        .collect(),
                };
                if let Some(call) = objects.get_mut(entry.call as usize) {
                    call.push(object);
                }
            }
            self.biped_object_features += update.objects.len() as u64;
            if record.after.bumped_object_index != -1 || record.after.elevator_object_index != -1 {
                self.bipeds_touched_objects += 1;
            }
            self.recent_physics.insert(record.sequence, (*record, objects.clone()));
            if self.recent_physics.len() > 8192 {
                let newest = record.sequence;
                self.recent_physics.retain(|&sequence, _| newest.wrapping_sub(sequence) < 4096);
            }
            let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) else { return };
            bsp.breakable_surfaces.set(Some(record.breakable_surfaces));
            let mut physics = record.before;
            let stand = (record.stand_test != 0).then_some(halo_map::biped::StandTest {
                collision_height_standing: record.stand_height,
                collision_radius: record.stand_radius,
            });
            halo_map::biped::biped_update_physics(bsp, &mut physics, stand, halo_map::biped::BipedObjects::Visited(&objects));
            bsp.breakable_surfaces.set(None);
            self.bipeds_replayed += 1;
            let bytes = |p: &halo_map::biped::BipedPhysics| {
                // SAFETY: BipedPhysics is repr(C) plain old data of 0xcc bytes
                unsafe { std::mem::transmute::<halo_map::biped::BipedPhysics, [u8; 0xcc]>(*p) }
            };
            if bytes(&physics) == bytes(&record.after) {
                self.bipeds_agreed += 1;
                let i = record.before.in_flags;
                let o = record.after.out_flags;
                let kind = if i & in_flags::FLYING != 0 {
                    0
                } else if i & in_flags::AIRBORNE != 0 {
                    1
                } else {
                    2
                };
                self.biped_kinds[kind] += 1;
                if o & out_flags::AIRBORNE == 0 {
                    self.biped_kinds[3] += 1;
                }
                if record.after.stick_surface_index != -1 {
                    self.biped_kinds[4] += 1;
                }
                if o & out_flags::CANNOT_STAND != 0 {
                    self.biped_kinds[5] += 1;
                }
                let bumped = record.after.bumped_object_index;
                if bumped != -1
                    && let Some((entry, _)) = update.objects.iter().find(|(e, _)| e.object_index as i32 == bumped)
                    && let Some(count) = self.bipeds_bumped_kinds.get_mut(entry.kind as usize)
                {
                    *count += 1;
                }
                if record.after.elevator_object_index != -1 {
                    self.bipeds_on_elevators += 1;
                }
            } else if self.bipeds_shown < 6 {
                self.bipeds_shown += 1;
                println!(
                    "  biped {:#x} differs:\n    before   {:?}\n    game     {:?}\n    halo_map {:?}",
                    record.biped_index, record.before, record.after, physics
                );
            }
        }
    }

    /// Replays the biped movement updates the game recorded with halo_map's
    /// `biped_update_moving`, from the movement state each began with, what
    /// it read and asked, and the objects and breakable surfaces its physics
    /// saw; compares the physics it asks for with the game's, and the state
    /// it leaves byte for byte.
    fn check_biped_moving(&mut self, structure_bsp_index: i32, records: &[halo_bridge::BipedMovingRecord]) {
        use halo_map::biped_moving::{BipedDefinition, MovingInputs, PhysicsWorld, PlayerInformation, biped_flags, state_flags};
        let cache = &self.cache;
        if self.player_information.is_none() {
            self.player_information = Some(PlayerInformation::load(cache).ok());
        }
        let Some(Some(player_information)) = self.player_information else { return };
        if !self.clusters.contains_key(&structure_bsp_index) {
            let loaded = halo_map::bsp::references(cache)
                .ok()
                .and_then(|r| r.into_iter().find(|r| r.index as i32 == structure_bsp_index))
                .and_then(|r| halo_map::bsp::Bsp::load(cache, &r).ok())
                .and_then(|b| halo_map::collision::StructureClusters::load(cache, &b).ok());
            self.clusters.insert(structure_bsp_index, loaded);
        }
        for record in records {
            self.moving_updates += 1;
            if record.structure_bsp_index != structure_bsp_index {
                self.moving_elsewhere += 1;
                continue;
            }
            let physics_record = if record.physics_record >= 0 {
                match self.recent_physics.get(&(record.physics_record as u32)) {
                    Some(found) => Some(found.clone()),
                    None => {
                        self.moving_missing_physics += 1;
                        continue;
                    }
                }
            } else {
                None
            };
            let definition = self
                .biped_definitions
                .entry(record.definition_index)
                .or_insert_with(|| BipedDefinition::load(cache, record.definition_index).ok())
                .clone();
            let Some(definition) = definition else { continue };
            let frame_info = (record.animation_index != -1)
                .then(|| {
                    halo_map::animation::frame_info(cache, record.animation_graph_index, record.animation_index, record.animation_frame_index)
                        .ok()
                        .flatten()
                })
                .flatten();
            let inputs = MovingInputs {
                biped_index: record.biped_index,
                dead: record.dead != 0,
                ignore_translation: record.ignore_translation != 0,
                suspended: record.suspended != 0,
                control_flags: record.control_flags,
                unit_animation_state: record.unit_animation_state,
                base_seat_index: record.base_seat_index,
                aiming_speed: record.aiming_speed,
                actor: record.actor != 0,
                player_index: record.player_index,
                throttle: record.throttle,
                body_stun: record.body_stun,
                recent_body_damage: record.recent_body_damage,
                desired_facing_vector: record.desired_facing_vector,
                aiming_vector: record.aiming_vector,
                bounding_center: record.bounding_center,
                frame_info,
                difficulty_infection_forms: record.difficulty_infection_forms,
                speed_multiplier: record.speed_multiplier,
                cinematic: record.cinematic != 0,
                double_speed: record.double_speed != 0,
                coop_player_collisions: record.coop_player_collisions != 0,
                actor_leaping: record.actor_leaping != 0,
                recorded_animation: record.recorded_animation != 0,
                actor_jump: (record.actor_jump_asked != 0).then_some((record.actor_jumped != 0, record.actor_jump_velocity)),
            };
            let no_objects = Vec::new();
            let (objects, stand) = match &physics_record {
                Some((physics, objects)) => (
                    objects,
                    (physics.stand_test != 0).then_some(halo_map::biped::StandTest {
                        collision_height_standing: physics.stand_height,
                        collision_radius: physics.stand_radius,
                    }),
                ),
                None => (&no_objects, None),
            };
            self.recent_moving.insert(record.sequence, (inputs, physics_record.clone()));
            if self.recent_moving.len() > 8192 {
                let newest = record.sequence;
                self.recent_moving.retain(|&sequence, _| newest.wrapping_sub(sequence) < 4096);
            }
            let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) else { return };
            bsp.breakable_surfaces.set(physics_record.as_ref().map(|(p, _)| p.breakable_surfaces));
            let Some(Some(bsp)) = self.bsps.get(&structure_bsp_index) else { return };
            let Some(Some(clusters)) = self.clusters.get(&structure_bsp_index) else { return };
            let world = PhysicsWorld { bsp, clusters, objects };
            let mut state = record.before;
            let outcome =
                halo_map::biped_moving::biped_update_moving(&world, &definition, &player_information, &inputs, &mut state, stand);
            if let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) {
                bsp.breakable_surfaces.set(None);
            }
            self.moving_replayed += 1;

            // the physics it asked for: the inputs of the game's (the rest of
            // the game's struct is uninitialised until the physics fills it)
            let mut physics_agrees = true;
            if let (Some(mine), Some((game, _))) = (outcome.physics, &physics_record) {
                let game = game.before;
                let bits3 = |v: [f32; 3]| v.map(f32::to_bits);
                physics_agrees = mine.biped_index == game.biped_index
                    && mine.in_flags == game.in_flags
                    && bits3(mine.position) == bits3(game.position)
                    && bits3(mine.forward) == bits3(game.forward)
                    && bits3(mine.aiming) == bits3(game.aiming)
                    && bits3(mine.velocity) == bits3(game.velocity)
                    && mine.crouch_velocity.to_bits() == game.crouch_velocity.to_bits()
                    && bits3(mine.movement_desired) == bits3(game.movement_desired)
                    && [
                        mine.movement_penalty,
                        mine.acceleration_maximum,
                        mine.airborne_acceleration_maximum,
                        mine.height,
                        mine.width,
                        mine.ground_tangential_velocity_max,
                        mine.ground_tangential_angle,
                        mine.minimum_normal_k,
                        mine.downhill_k0,
                        mine.downhill_k1,
                        mine.downhill_velocity_scale,
                        mine.uphill_k0,
                        mine.uphill_k1,
                        mine.uphill_velocity_scale,
                    ]
                    .map(f32::to_bits)
                        == [
                            game.movement_penalty,
                            game.acceleration_maximum,
                            game.airborne_acceleration_maximum,
                            game.height,
                            game.width,
                            game.ground_tangential_velocity_max,
                            game.ground_tangential_angle,
                            game.minimum_normal_k,
                            game.downhill_k0,
                            game.downhill_k1,
                            game.downhill_velocity_scale,
                            game.uphill_k0,
                            game.uphill_k1,
                            game.uphill_velocity_scale,
                        ]
                        .map(f32::to_bits)
                    && game.ground_plane.map(f32::to_bits) == mine.ground_plane.map(f32::to_bits)
                    && mine.existing_support_surface_index == game.existing_support_surface_index;
                self.moving_physics_compared += 1;
                if physics_agrees {
                    self.moving_physics_agreed += 1;
                }
            } else if outcome.physics.is_some() != physics_record.is_some() {
                physics_agrees = false;
            }

            let bytes = |s: &halo_map::biped_moving::MovingState| {
                // SAFETY: MovingState is repr(C) plain old data of 136 bytes
                unsafe { std::mem::transmute::<halo_map::biped_moving::MovingState, [u8; 136]>(*s) }
            };
            // (a biped that fell outside the world, erased by the game's
            // falling damage, which isn't ported: its object made invisible)
            if record.after.object_flags & 1 != 0 && record.before.object_flags & 1 == 0 {
                state.object_flags |= 1;
                self.moving_erased += 1;
            }
            if physics_agrees && bytes(&state) == bytes(&record.after) && outcome.aborted_animation == (record.aborted_animation != 0) {
                self.moving_agreed += 1;
                let b = &record.before;
                let a = &record.after;
                let k = &mut self.moving_kinds;
                let flying = definition.flags & biped_flags::FLYING != 0 && !inputs.dead;
                if flying {
                    k[2] += 1;
                } else if definition.flags & biped_flags::USES_PLAYER_PHYSICS != 0 && inputs.unit_animation_state != 28 {
                    k[0] += 1;
                } else {
                    k[1] += 1;
                }
                if a.forward != b.forward && frame_info.is_some_and(|f| f.dyaw != 0.0) {
                    k[3] += 1;
                }
                if a.biped_flags & state_flags::AIRBORNE != 0 && a.jump_recovery_timer == 0 && b.biped_flags & state_flags::AIRBORNE == 0 {
                    k[4] += 1;
                }
                if a.landing != b.landing || (a.landing_recovery_counter == 0 && a.landing_recovery_time != b.landing_recovery_time) {
                    k[5] += 1;
                }
                if outcome.aborted_animation {
                    k[6] += 1;
                }
                if a.crouch != b.crouch {
                    k[7] += 1;
                }
            } else if self.moving_shown < 6 {
                self.moving_shown += 1;
                println!(
                    "  biped movement {:#x} differs (physics input agrees: {physics_agrees}; aborted game {} halo_map {}):\n    inputs   {:?}\n    before   {:?}\n    game     {:?}\n    halo_map {:?}",
                    record.biped_index, record.aborted_animation, outcome.aborted_animation, inputs, record.before, record.after, state
                );
                if !physics_agrees
                    && let (Some(mine), Some((game, _))) = (outcome.physics, &physics_record)
                {
                    println!("    physics game     {:?}\n    physics halo_map {:?}", game.before, mine);
                }
            }
        }
    }

    /// Replays the biped updates the game recorded (an unparented biped's,
    /// up to its animation update) with halo_map's `biped_update`: turning,
    /// the movement (with what its movement record read), and the dead,
    /// airborne and landing updates; compares the state byte for byte.
    fn check_biped_updates(&mut self, structure_bsp_index: i32, records: &[halo_bridge::BipedUpdateRecord]) {
        use halo_map::biped_moving::{BipedDefinition, PhysicsWorld, biped_flags, state_flags};
        let Some(Some(player_information)) = self.player_information else { return };
        for record in records {
            self.update_updates += 1;
            if record.structure_bsp_index != structure_bsp_index {
                self.update_elsewhere += 1;
                continue;
            }
            let Some((moving_inputs, physics_record)) =
                (record.moving_record >= 0).then(|| self.recent_moving.get(&(record.moving_record as u32)).cloned()).flatten()
            else {
                self.update_missing += 1;
                continue;
            };
            let cache = &self.cache;
            let definition = self
                .biped_definitions
                .entry(record.definition_index)
                .or_insert_with(|| BipedDefinition::load(cache, record.definition_index).ok())
                .clone();
            let Some(definition) = definition else { continue };
            let inputs = halo_map::biped_update::UpdateInputs {
                dead: record.dead != 0,
                control_flags: record.control_flags,
                aim_without_turning: record.aim_without_turning != 0,
                unit_animation_state: record.unit_animation_state,
                base_seat_index: record.base_seat_index,
                enhanced_animation: record.enhanced_animation != 0,
                limp_body: record.limp_body != 0,
                player_index: record.player_index,
                airborne_seed: (record.airborne_seed_taken != 0).then_some(record.airborne_seed),
            };
            let no_objects = Vec::new();
            let (objects, stand) = match &physics_record {
                Some((physics, objects)) => (
                    objects,
                    (physics.stand_test != 0).then_some(halo_map::biped::StandTest {
                        collision_height_standing: physics.stand_height,
                        collision_radius: physics.stand_radius,
                    }),
                ),
                None => (&no_objects, None),
            };
            let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) else { return };
            bsp.breakable_surfaces.set(physics_record.as_ref().map(|(p, _)| p.breakable_surfaces));
            let (Some(Some(bsp)), Some(Some(clusters))) = (self.bsps.get(&structure_bsp_index), self.clusters.get(&structure_bsp_index)) else {
                return;
            };
            let world = PhysicsWorld { bsp, clusters, objects };
            let mut state = record.before;
            halo_map::biped_update::biped_update(&world, &definition, &player_information, &inputs, &moving_inputs, stand, &[], &mut state);
            if let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) {
                bsp.breakable_surfaces.set(None);
            }
            self.update_replayed += 1;
            let bytes = |s: &halo_map::biped_update::UpdateState| {
                // SAFETY: UpdateState is repr(C) plain old data of 164 bytes
                unsafe { std::mem::transmute::<halo_map::biped_update::UpdateState, [u8; 164]>(*s) }
            };
            // (erased by the game for falling outside the world, as above)
            if record.after.moving.object_flags & 1 != 0 && record.before.moving.object_flags & 1 == 0 {
                state.moving.object_flags |= 1;
                self.update_erased += 1;
            }
            if bytes(&state) == bytes(&record.after) {
                self.update_agreed += 1;
                let (b, a) = (&record.before.moving, &record.after.moving);
                let k = &mut self.update_kinds;
                if inputs.dead {
                    k[0] += 1;
                } else if a.biped_flags & state_flags::AIRBORNE != 0 {
                    k[1] += 1;
                } else if b.landing != -1 {
                    k[2] += 1;
                }
                if definition.flags & biped_flags::FLYING != 0 && !inputs.dead {
                    k[3] += 1;
                } else if record.after.biped_state == 1 && a.forward != b.forward {
                    k[4] += 1;
                }
                if a.animation_state_desired == 2 || a.animation_state_desired == 3 {
                    k[5] += 1;
                }
                if inputs.airborne_seed.is_some() && a.angular_velocity != b.angular_velocity {
                    k[6] += 1;
                }
            } else if self.update_shown < 6 {
                self.update_shown += 1;
                println!(
                    "  biped update {:#x} differs:\n    inputs   {:?}\n    moving   {:?}\n    before   {:?}\n    game     {:?}\n    halo_map {:?}",
                    record.biped_index, inputs, moving_inputs, record.before, record.after, state
                );
            }
        }
    }

    /// Replays the unit animation updates of bipeds the game recorded with
    /// halo_map's `unit_update_animation`, from the state each began with
    /// and the random seed then; compares the state, the side effects and
    /// what it returned.
    fn check_unit_animations(&mut self, records: &[halo_bridge::UnitAnimationRecord]) {
        use halo_map::biped_moving::{BipedDefinition, biped_flags};
        for record in records {
            self.animation_updates += 1;
            let cache = &self.cache;
            let definition = self
                .biped_definitions
                .entry(record.definition_index)
                .or_insert_with(|| BipedDefinition::load(cache, record.definition_index).ok())
                .clone();
            let Some(definition) = definition else { continue };
            let Some(unit_graph_index) = definition.animation_graph else { continue };
            for index in [unit_graph_index, record.before.object_animation_graph_index] {
                if index != u32::MAX && !self.animation_graphs.contains_key(&index) {
                    let loaded = halo_map::unit_animation::AnimationGraph::load(cache, index).ok();
                    self.animation_graphs.insert(index, loaded);
                }
            }
            let end = record.weapon_label.iter().position(|&b| b == 0).unwrap_or(32);
            let inputs = halo_map::unit_animation::UnitAnimationInputs {
                state_desired: record.state_desired,
                crouching: record.crouching != 0,
                desired_state: record.desired_state,
                dead: record.dead != 0,
                parented: record.parented != 0,
                player: record.player != 0,
                force_alert: record.force_alert != 0,
                flaming: record.flaming != 0,
                magic_base_seat_index: record.magic_base_seat_index,
                magic_seat_index: record.magic_seat_index,
                at_rest: record.at_rest != 0,
                biped: true,
                airborne: record.airborne != 0,
                weapon_label: String::from_utf8_lossy(&record.weapon_label[..end]).into_owned(),
                random_seed: record.random_seed,
                destroyed_after_dying: definition.unit_flags & (1 << 1) != 0,
                has_no_dying_airborne: definition.flags & biped_flags::HAS_NO_DYING_AIRBORNE != 0,
            };
            let graphs = &self.animation_graphs;
            let graph = |index: u32| graphs.get(&index).and_then(Option::as_ref);
            let mut state = record.before;
            let (result, effects) = halo_map::unit_animation::unit_update_animation(unit_graph_index, &graph, &inputs, &mut state);
            // (a destroyed unit's state isn't read after)
            let compare_state = record.side_effects & halo_map::unit_animation::side_effects::DESTROYED == 0;
            if (!compare_state || state == record.after) && effects == record.side_effects && result == record.result {
                self.animation_agreed += 1;
                let (b, a) = (&record.before, &record.after);
                let k = &mut self.animation_kinds;
                if a.state != b.state {
                    k[0] += 1;
                }
                if a.seat_index != b.seat_index || a.weapon_index != b.weapon_index || a.weapon_type_index != b.weapon_type_index {
                    k[1] += 1;
                }
                if a.object_animation_frame_index == 0 && b.object_animation_frame_index != 0 && a.state == b.state {
                    k[2] += 1;
                }
                if effects != 0 {
                    k[3] += 1;
                    self.animation_side_effects |= effects;
                }
                if result != 0 {
                    k[4] += 1;
                }
            } else if self.animation_shown < 8 {
                self.animation_shown += 1;
                println!(
                    "  unit animation {:#x} differs (effects game {:#x} halo_map {:#x}, result game {} halo_map {}):\n    inputs   {:?}\n    before   {:?}\n    game     {:?}\n    halo_map {:?}",
                    record.unit_index, record.side_effects, effects, record.result, result, inputs, record.before, record.after, state
                );
            }
        }
    }

    /// Replays the sampled poses the game recorded with halo_map's posing:
    /// the orientations from the animation's frame (or the model's pose)
    /// and the object's scale, compared before the object type's preprocess;
    /// and, from the game's preprocessed orientations, the interpolation and
    /// the node matrices.
    fn check_poses(&mut self, poses: &[(halo_bridge::PoseRecord, Vec<halo_bridge::PoseNodeRecord>, bool)]) {
        use halo_map::pose::{self, Orientation, Placement};
        for (record, nodes, complete) in poses {
            self.poses += 1;
            if !complete {
                self.poses_incomplete += 1;
                continue;
            }
            let cache = &self.cache;
            let definition = self
                .pose_definitions
                .entry(record.definition_index)
                .or_insert_with(|| pose::PoseObjectDefinition::load(cache, record.definition_index).ok())
                .clone();
            let Some(definition) = definition else { continue };
            let Some(model_index) = definition.model else { continue };
            if let Some(graph_index) = definition.animation_graph {
                self.load_graph_animations(graph_index);
            }
            let cache = &self.cache;
            if !self.pose_models.contains_key(&model_index) {
                self.pose_models.insert(model_index, pose::PoseModel::load(cache, model_index).ok());
            }
            let Some(Some(model)) = self.pose_models.get(&model_index) else { continue };
            let node_count = record.node_count as usize;
            if nodes.len() != node_count || model.nodes.len().min(pose::MAXIMUM_NODES) != node_count {
                continue;
            }
            if let Some(count) = self.pose_types.get_mut(record.kind as usize) {
                *count += 1;
            }

            // the orientations from the animation (or the model) and scale
            let animated = record.frame_index != -1 && record.animation_graph_index != u32::MAX && record.animation_index != -1;
            if animated {
                self.pose_animations
                    .entry((record.animation_graph_index, record.animation_index))
                    .or_insert_with(|| pose::PoseAnimation::load(cache, record.animation_graph_index, record.animation_index).ok().flatten());
            }
            let animation = animated.then(|| self.pose_animations.get(&(record.animation_graph_index, record.animation_index))).flatten().and_then(|a| a.as_ref());
            let mut orientations = match animation {
                Some(animation) => pose::animation_node_orientations(model, animation, record.frame_index),
                None => pose::model_node_orientations(model),
            };
            if let Some(graph_index) = definition.animation_graph {
                let animations = &self.pose_animations;
                let animation = |index: i16| animations.get(&(graph_index, index)).and_then(|a| a.as_ref());
                pose::object_overlays_apply(
                    &definition,
                    &animation,
                    node_count,
                    record.function_values,
                    record.object_index.wrapping_add(record.game_time as u32),
                    &mut orientations,
                );
            }
            if record.scale > 0.0 && !orientations.is_empty() {
                let o = &mut orientations[0];
                o.scale *= record.scale;
                o.translation = [o.translation[0] * record.scale, o.translation[1] * record.scale, o.translation[2] * record.scale];
            }
            {
                self.pose_animated[0] += 1;
                if !definition.overlays.is_empty() {
                    self.pose_animated[2] += 1;
                }
                let same = orientations.len() == node_count
                    && orientations.iter().zip(nodes).all(|(o, n)| o.to_floats().map(f32::to_bits) == n.animated.map(f32::to_bits));
                if same {
                    self.pose_animated[1] += 1;
                    match animation {
                        Some(a) if a.flags & 1 != 0 => self.pose_animated[3] += 1,
                        None => self.pose_animated[4] += 1,
                        _ => {}
                    }
                } else if self.pose_shown < 6 {
                    self.pose_shown += 1;
                    let first = orientations.iter().zip(nodes).position(|(o, n)| o.to_floats().map(f32::to_bits) != n.animated.map(f32::to_bits));
                    println!(
                        "  pose {:#x} (type {}, definition {:#x}) orientations differ at node {first:?} of {}: frame {} of animation {} (compressed {:?}); game {:?}; halo_map {:?}",
                        record.object_index,
                        record.kind,
                        record.definition_index,
                        node_count,
                        record.frame_index,
                        record.animation_index,
                        animation.map(|a| a.flags & 1 != 0),
                        first.map(|i| nodes[i].animated),
                        first.map(|i| orientations[i].to_floats())
                    );
                }
            }

            // the matrices, from the game's preprocessed orientations
            let mut target: Vec<Orientation> = nodes.iter().map(|n| Orientation::from_floats(n.preprocessed)).collect();
            if record.interpolation_frame_count > 0 {
                let original: Vec<Orientation> = nodes.iter().map(|n| Orientation::from_floats(n.original)).collect();
                pose::interpolate_node_orientations(&original, &mut target, record.interpolation_frame_index, record.interpolation_frame_count);
            }
            let m = |f: [f32; 13]| halo_map::model::Matrix4x3 {
                scale: f[0],
                forward: [f[1], f[2], f[3]],
                left: [f[4], f[5], f[6]],
                up: [f[7], f[8], f[9]],
                position: [f[10], f[11], f[12]],
            };
            let placement = Placement {
                position: record.position,
                forward: record.forward,
                up: record.up,
                mirrored: record.object_flags & (1 << 12) != 0,
                world_relative: record.world_relative != 0,
                center_of_mass: definition.center_of_mass,
                origin_offset: definition.origin_offset,
                parent: (record.has_parent != 0).then(|| (m(record.parent_node_matrix), record.parent_mirrored != 0)),
            };
            let matrices = pose::node_matrices(model, &target, &placement);
            let bits = |x: &halo_map::model::Matrix4x3| {
                [
                    x.scale, x.forward[0], x.forward[1], x.forward[2], x.left[0], x.left[1], x.left[2], x.up[0], x.up[1], x.up[2], x.position[0],
                    x.position[1], x.position[2],
                ]
                .map(f32::to_bits)
            };
            self.pose_matrices[0] += 1;
            let same = matrices.iter().zip(nodes).all(|(x, n)| bits(x) == n.matrix.map(f32::to_bits));
            if same {
                self.pose_matrices[1] += 1;
                if record.interpolation_frame_count > 0 {
                    self.pose_matrices[2] += 1;
                }
                if record.has_parent != 0 {
                    self.pose_matrices[3] += 1;
                }
            } else if self.pose_shown < 12 {
                self.pose_shown += 1;
                let first = matrices.iter().zip(nodes).position(|(x, n)| bits(x) != n.matrix.map(f32::to_bits));
                println!(
                    "  pose {:#x} (type {}) matrices differ at node {first:?} of {} (interpolating {}/{}, parent {}): game {:?}; halo_map {:?}",
                    record.object_index,
                    record.kind,
                    node_count,
                    record.interpolation_frame_index,
                    record.interpolation_frame_count,
                    record.has_parent,
                    first.map(|i| nodes[i].matrix),
                    first.map(|i| matrices[i])
                );
            }

            // the type's preprocess, from the game's animated orientations
            self.check_preprocess(record, nodes, model_index, &definition);
        }
    }

    /// Replays players' melee steps (`biped_update`'s, and the melee action
    /// they start, `unit_animation_start_action`).
    fn check_melees(&mut self, records: &[halo_bridge::MeleeRecord]) {
        use halo_map::biped_moving::BipedDefinition;
        use halo_map::biped_update::{MeleeWeapon, PlayerMeleeInputs, player_melee};
        use halo_map::unit_animation::{UnitAnimationState, action, unit_animation_start_action};
        for record in records {
            let cache = &self.cache;
            let definition = self
                .biped_definitions
                .entry(record.definition_index)
                .or_insert_with(|| BipedDefinition::load(cache, record.definition_index).ok())
                .clone();
            let Some(definition) = definition else { continue };
            let Some(graph_index) = definition.animation_graph else { continue };
            if !self.animation_graphs.contains_key(&graph_index) {
                self.animation_graphs.insert(graph_index, halo_map::unit_animation::AnimationGraph::load(cache, graph_index).ok());
            }
            let weapon = if record.weapon_definition_index == u32::MAX {
                None
            } else {
                let loaded = self
                    .melee_weapons
                    .entry(record.weapon_definition_index)
                    .or_insert_with(|| MeleeWeapon::load(cache, record.weapon_definition_index).ok());
                let Some(weapon) = *loaded else { continue };
                Some((weapon, record.weapon_overcharged != 0))
            };
            let Some(Some(graph)) = self.animation_graphs.get(&graph_index) else { continue };
            let inputs = PlayerMeleeInputs {
                use_equipment: record.use_equipment != 0,
                in_seat: record.in_seat != 0,
                zoomed: record.zoomed != 0,
                weapon,
            };
            let (mut ticks, mut attack_tick) = (record.ticks_before, record.attack_tick_before);
            let outcome = player_melee(&inputs, &mut ticks, &mut attack_tick);
            let mut state = UnitAnimationState {
                seat_index: record.seat_index,
                weapon_index: record.weapon_index,
                weapon_type_index: record.weapon_type_index,
                action: record.action_before,
                action_animation_index: record.action_animation_before[0],
                action_animation_frame_index: record.action_animation_before[1],
                ..Default::default()
            };
            let mut seed = record.random_seed_before;
            if outcome.started {
                unit_animation_start_action(graph, record.enhanced != 0, &mut seed, &mut state, action::MELEE);
            }
            self.melees[0] += 1;
            let same = ticks == record.ticks_after
                && attack_tick == record.attack_tick_after
                && outcome.started == (record.started != 0)
                && outcome.damaged == (record.damaged != 0)
                && (!outcome.started || seed == record.random_seed_after_start)
                && state.action == record.action_after
                && [state.action_animation_index, state.action_animation_frame_index] == record.action_animation_after;
            if same {
                let k = &mut self.melees;
                k[1] += 1;
                k[2] += (outcome.started && weapon.is_some()) as u64;
                k[3] += (outcome.started && weapon.is_none()) as u64;
                k[4] += outcome.damaged as u64;
                k[5] += (record.ticks_before != 0) as u64;
            } else if self.melees_shown < 8 {
                self.melees_shown += 1;
                println!(
                    "  melee of {:#x} differs: record {:?}\n    halo_map ticks {} attack {} {:?} seed {:#x} action {} animation {} frame {} (weapon {:?})",
                    record.biped_index,
                    record,
                    ticks,
                    attack_tick,
                    outcome,
                    seed,
                    state.action,
                    state.action_animation_index,
                    state.action_animation_frame_index,
                    weapon
                );
            }
        }
    }

    /// An object the game recorded, as collision features see it.
    fn feature_object(&mut self, entry: &halo_bridge::FeatureObjectRecord, matrices: &[[f32; 13]]) -> halo_map::features::FeatureObject {
        let cache = &self.cache;
        let definition = halo_map::TagRef { group: 0, datum: Some(entry.definition_index) };
        let model = self
            .models
            .entry(entry.definition_index)
            .or_insert_with(|| {
                let datum = halo_map::objects::object_collision_model(cache, definition).ok()??.datum?;
                halo_map::collision::CollisionModel::load(cache, datum).ok().map(std::sync::Arc::new)
            })
            .clone();
        halo_map::features::FeatureObject {
            index: entry.object_index as i32,
            kind: entry.kind,
            flags: entry.flags,
            dead: entry.dead != 0,
            bounding_center: entry.bounding_center,
            bounding_radius: entry.bounding_radius,
            parent: entry.parent_object_index,
            velocity: entry.translational_velocity,
            elevator: entry.elevator != 0,
            biped: (entry.kind == halo_map::features::OBJECT_TYPE_BIPED).then_some(halo_map::features::BipedPill {
                passes_through_bipeds: entry.biped_passes_through != 0,
                player: entry.player_index != -1,
                seated: entry.parent_seat_index != -1,
                base: entry.pill_base,
                height: entry.pill_height,
                width: entry.pill_width,
            }),
            model,
            region_permutations: entry.region_permutations,
            matrices: matrices
                .iter()
                .map(|m| halo_map::model::Matrix4x3 {
                    scale: m[0],
                    forward: [m[1], m[2], m[3]],
                    left: [m[4], m[5], m[6]],
                    up: [m[7], m[8], m[9]],
                    position: [m[10], m[11], m[12]],
                })
                .collect(),
        }
    }

    fn physics_definition(&mut self, definition_index: u32) -> Option<std::sync::Arc<halo_map::vehicle_physics::PhysicsDefinition>> {
        let cache = &self.cache;
        self.physics_definitions
            .entry(definition_index)
            .or_insert_with(|| {
                halo_map::vehicle_physics::PhysicsDefinition::load_for_object(cache, definition_index).ok().flatten().map(std::sync::Arc::new)
            })
            .clone()
    }

    /// The world a recorded vehicle physics update met: the objects each
    /// features call visited, the objects once each, the other vehicles.
    #[allow(clippy::type_complexity)]
    fn vehicle_physics_objects(
        &mut self,
        update: &halo_bridge::VehiclePhysicsUpdate,
    ) -> (Vec<Vec<halo_map::features::FeatureObject>>, Vec<halo_map::collision::WorldObject>, Vec<halo_map::vehicle_physics::OtherVehicle>) {
        let record = &update.record;
        let mut feature_objects: Vec<Vec<halo_map::features::FeatureObject>> = vec![Vec::new(); record.feature_calls as usize];
        let world_objects = self.world_objects(&update.objects, Some(&mut feature_objects));
        let mut others = Vec::new();
        for other in &record.others[..(record.other_count as usize).min(8)] {
            if let Some(physics) = self.physics_definition(other.definition_index) {
                others.push(halo_map::vehicle_physics::OtherVehicle {
                    index: other.object_index,
                    at_rest: other.at_rest != 0,
                    physics,
                    origin: other.origin,
                    forward: other.forward,
                    up: other.up,
                });
            }
        }
        (feature_objects, world_objects, others)
    }

    /// Recorded objects for the broad phase (each once, as first seen),
    /// and, given, each into its features call's list.
    fn world_objects(
        &mut self,
        objects: &[(halo_bridge::FeatureObjectRecord, Vec<[f32; 13]>)],
        mut feature_objects: Option<&mut Vec<Vec<halo_map::features::FeatureObject>>>,
    ) -> Vec<halo_map::collision::WorldObject> {
        let mut seen = std::collections::HashSet::new();
        let mut world_objects = Vec::new();
        for (entry, matrices) in objects {
            let object = self.feature_object(entry, matrices);
            if let Some(calls) = feature_objects.as_deref_mut()
                && let Some(call) = calls.get_mut(entry.call as usize)
            {
                call.push(object.clone());
            }
            if seen.insert(entry.object_index) {
                world_objects.push(halo_map::collision::WorldObject {
                    index: entry.object_index,
                    kind: entry.kind,
                    flags: entry.flags,
                    bounding_center: entry.bounding_center,
                    bounding_radius: entry.bounding_radius,
                    parent: entry.parent_object_index as u32,
                    location_cluster: entry.location_cluster,
                    region_permutations: entry.region_permutations,
                    matrices: object.matrices,
                    model: object.model,
                    physics: None,
                });
            }
        }
        world_objects
    }

    /// Replays vehicle updates (`vehicle_update`, to its animation): the
    /// control code from the driver's controls, the physics it hands them to
    /// (with the world its nested physics record met), the suspensions and
    /// the ticks; compares the controls handed to the physics and the state
    /// it leaves byte for byte.
    fn check_vehicle_updates(&mut self, structure_bsp_index: i32, updates: &[halo_bridge::VehicleUpdate]) {
        use halo_map::vehicle_physics as vp;
        use halo_map::vehicle_update as vu;
        let cache = &self.cache;
        if !self.vehicle_bounds.contains_key(&structure_bsp_index) {
            let bounds = halo_map::bsp::references(cache)
                .ok()
                .and_then(|r| r.into_iter().find(|r| r.index as i32 == structure_bsp_index))
                .and_then(|reference| halo_map::bsp::Bsp::load(cache, &reference).ok())
                .and_then(|bsp| vu::structure_vehicle_bounds(&bsp).ok());
            self.vehicle_bounds.insert(structure_bsp_index, bounds);
        }
        let Some(Some((floor, ceiling))) = self.vehicle_bounds.get(&structure_bsp_index).copied() else { return };
        for update in updates {
            let record = &update.record;
            self.vehicle_updates[0] += 1;
            if record.structure_bsp_index != structure_bsp_index {
                self.vehicle_updates[3] += 1;
                continue;
            }
            let physics_update = if record.physics_record != 0 {
                match self.recent_vehicle_physics.get(&(record.physics_record - 1)) {
                    Some(p) => Some(p.clone()),
                    None => {
                        self.vehicle_updates[4] += 1;
                        continue;
                    }
                }
            } else {
                None
            };
            if !update.complete || physics_update.as_ref().is_some_and(|p| !p.complete) {
                self.vehicle_updates[4] += 1;
                continue;
            }
            let cache = &self.cache;
            let definition = self
                .vehicle_definitions
                .entry(record.definition_index)
                .or_insert_with(|| vu::VehicleDefinition::load(cache, record.definition_index).ok().map(std::sync::Arc::new))
                .clone();
            let Some(definition) = definition else { continue };
            let physics = self.physics_definition(record.definition_index);
            let suspension_objects = self.world_objects(&update.objects, None);
            let physics_world = physics_update.as_ref().map(|p| self.vehicle_physics_objects(p));
            let powered_base: Vec<vp::PoweredMassPoint> = physics_update
                .as_ref()
                .map(|p| {
                    p.record.powered[..(p.record.powered_count as usize).min(8)]
                        .iter()
                        .map(|q| vp::PoweredMassPoint {
                            ground_friction_velocity: q.values[0],
                            water_friction_velocity: q.values[1],
                            air_friction_velocity: q.values[2],
                            water_lift_ratio: q.values[3],
                            air_lift_ratio: q.values[4],
                            thrust_fraction: q.values[5],
                            antigrav_fraction: q.values[6],
                            rotation: q.rotation,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let inputs = vu::VehicleUpdateInputs {
                index: record.vehicle_index,
                parented: record.parented != 0,
                throttle: record.throttle,
                control_flags: record.control_flags,
                desired_facing: record.desired_facing,
                seat_power: record.seat_power,
                driven_by_ai: record.driven_by_ai != 0,
                vehicle_floor: floor,
                vehicle_ceiling: ceiling,
                bounding_center_after: physics_update.as_ref().map_or(record.after.bounding_center, |p| p.record.after.bounding_center),
                powered_base: &powered_base,
            };
            let datum = |bytes: &[u8; 88]| {
                // SAFETY: VehicleDatum is repr(C) plain old data of 88 bytes
                unsafe { std::mem::transmute::<[u8; 88], vu::VehicleDatum>(*bytes) }
            };
            let b = &record.before;
            let mut state = vu::VehicleUpdateState {
                position: b.position,
                forward: b.forward,
                up: b.up,
                translational_velocity: b.translational_velocity,
                angular_velocity: b.angular_velocity,
                object_flags: b.object_flags,
                location_leaf: b.location_leaf,
                location_cluster: b.location_cluster,
                vehicle: datum(&b.vehicle),
            };
            let breakables = physics_update.as_ref().map_or(record.breakable_surfaces, |p| p.record.breakable_surfaces);
            let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) else { return };
            bsp.breakable_surfaces.set(Some(breakables));
            let (Some(Some(bsp)), Some(Some(clusters))) = (self.bsps.get(&structure_bsp_index), self.clusters.get(&structure_bsp_index)) else {
                return;
            };
            let suspension_world = halo_map::collision::World { collision: bsp, clusters, objects: &suspension_objects, source: None };
            let physics_collision_world =
                physics_world.as_ref().map(|(_, objects, _)| halo_map::collision::World { collision: bsp, clusters, objects, source: None });
            let materials = self.material_physics.as_deref().unwrap_or(&[]);
            let env = vu::VehicleUpdateWorld {
                physics: match (&physics_world, &physics_collision_world) {
                    (Some((feature_objects, _, others)), Some(world)) => {
                        Some(vp::VehiclePhysicsWorld { world, feature_objects, materials, others })
                    }
                    _ => None,
                },
                world: &suspension_world,
            };
            let outcome = vu::vehicle_update(&definition, physics.as_deref(), &mut state, &inputs, &env);
            if let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) {
                bsp.breakable_surfaces.set(None);
            }
            self.vehicle_updates[1] += 1;
            // the controls handed to the physics
            let bits = |v: [f32; 3]| v.map(f32::to_bits);
            let controls_agree = match &physics_update {
                None => !outcome.physics_ran,
                Some(p) => {
                    let r = &p.record;
                    let powered_agree = match &outcome.powered {
                        None => r.powered_count == 0,
                        Some(ours) => {
                            ours.len() == r.powered_count as usize
                                && ours.iter().zip(&r.powered).all(|(o, g)| {
                                    let values = [
                                        o.ground_friction_velocity,
                                        o.water_friction_velocity,
                                        o.air_friction_velocity,
                                        o.water_lift_ratio,
                                        o.air_lift_ratio,
                                        o.thrust_fraction,
                                        o.antigrav_fraction,
                                    ];
                                    values.map(f32::to_bits) == g.values.map(f32::to_bits) && o.rotation.map(f32::to_bits) == g.rotation.map(f32::to_bits)
                                })
                        }
                    };
                    let magic_agree = match outcome.magic {
                        None => r.has_magic == 0,
                        Some((force, torque)) => r.has_magic != 0 && bits(force) == bits(r.magic_force) && bits(torque) == bits(r.magic_torque),
                    };
                    outcome.physics_ran && powered_agree && magic_agree
                }
            };
            let a = &record.after;
            let datum_bytes = |d: &vu::VehicleDatum| {
                // SAFETY: as above
                unsafe { std::mem::transmute::<vu::VehicleDatum, [u8; 88]>(*d) }
            };
            let same = bits(state.position) == bits(a.position)
                && bits(state.forward) == bits(a.forward)
                && bits(state.up) == bits(a.up)
                && bits(state.translational_velocity) == bits(a.translational_velocity)
                && bits(state.angular_velocity) == bits(a.angular_velocity)
                && state.object_flags == a.object_flags
                && state.location_leaf == a.location_leaf
                && state.location_cluster == a.location_cluster
                && datum_bytes(&state.vehicle) == a.vehicle;
            if controls_agree {
                self.vehicle_updates[5] += 1;
            }
            if same && controls_agree {
                let k = &mut self.vehicle_updates;
                k[2] += 1;
                k[6] += outcome.physics_ran as u64;
                k[7] += outcome.stopping as u64;
                k[8] += (record.parented != 0) as u64;
                k[9] += outcome.upended as u64;
                k[10] += (record.throttle != [0.0; 3]) as u64;
                if let Some(count) = self.vehicle_update_types.get_mut(definition.vehicle_type.max(0) as usize) {
                    *count += 1;
                }
            } else if self.vehicle_updates_shown < 8 {
                self.vehicle_updates_shown += 1;
                println!(
                    "  vehicle update {:#x} (type {}, definition {:#x}) differs (controls agree {controls_agree}; physics ran {}):\n    game     {:?} {:?}\n    halo_map {:?}\n    powered {:?} magic {:?}\n    game powered {:?} magic {:?}",
                    record.vehicle_index,
                    definition.vehicle_type,
                    record.definition_index,
                    outcome.physics_ran,
                    datum(&a.vehicle),
                    (a.position, a.translational_velocity, a.object_flags),
                    (state.vehicle, state.position, state.translational_velocity, state.object_flags),
                    outcome.powered,
                    outcome.magic,
                    physics_update.as_ref().map(|p| p.record.powered[..(p.record.powered_count as usize).min(8)].to_vec()),
                    physics_update.as_ref().map(|p| (p.record.has_magic, p.record.magic_force, p.record.magic_torque)),
                );
            }
        }
    }

    /// Replays vehicles' physics updates (`physics_update`) from the state
    /// each began with, what its vehicle code gave it, and the objects and
    /// vehicles it met as they were then; compares the state it leaves and
    /// its mass points byte for byte.
    fn check_vehicle_physics(&mut self, structure_bsp_index: i32, updates: &[halo_bridge::VehiclePhysicsUpdate]) {
        use halo_map::vehicle_physics as vp;
        let cache = &self.cache;
        fn load(cache: &halo_map::CacheFile, structure_bsp_index: i32) -> Option<halo_map::bsp::Bsp<'_>> {
            let reference = halo_map::bsp::references(cache).ok()?.into_iter().find(|r| r.index as i32 == structure_bsp_index)?;
            halo_map::bsp::Bsp::load(cache, &reference).ok()
        }
        if !self.bsps.contains_key(&structure_bsp_index) {
            let loaded = load(cache, structure_bsp_index).and_then(|b| halo_map::collision::CollisionBsp::load(&b).ok());
            self.bsps.insert(structure_bsp_index, loaded);
        }
        if !self.clusters.contains_key(&structure_bsp_index) {
            let loaded = load(cache, structure_bsp_index).and_then(|b| halo_map::collision::StructureClusters::load(cache, &b).ok());
            self.clusters.insert(structure_bsp_index, loaded);
        }
        if self.material_physics.is_none() {
            self.material_physics = Some(vp::load_material_physics(cache).unwrap_or_default());
        }
        for update in updates {
            let record = &update.record;
            self.vehicle_physics[0] += 1;
            if record.old_physics != 0 {
                self.vehicle_physics[4] += 1;
            }
            if !update.complete {
                self.vehicle_physics[5] += 1;
                continue;
            }
            if record.structure_bsp_index != structure_bsp_index {
                self.vehicle_physics[6] += 1;
                continue;
            }
            let Some(physics) = self.physics_definition(record.definition_index) else { continue };
            let (feature_objects, world_objects, others) = self.vehicle_physics_objects(update);
            self.recent_vehicle_physics.insert(record.sequence, update.clone());
            if self.recent_vehicle_physics.len() > 1024 {
                let newest = record.sequence;
                self.recent_vehicle_physics.retain(|&sequence, _| newest.wrapping_sub(sequence) < 512);
            }
            let powered: Vec<vp::PoweredMassPoint> = record.powered[..(record.powered_count as usize).min(8)]
                .iter()
                .map(|p| vp::PoweredMassPoint {
                    ground_friction_velocity: p.values[0],
                    water_friction_velocity: p.values[1],
                    air_friction_velocity: p.values[2],
                    water_lift_ratio: p.values[3],
                    air_lift_ratio: p.values[4],
                    thrust_fraction: p.values[5],
                    antigrav_fraction: p.values[6],
                    rotation: p.rotation,
                })
                .collect();
            let driven = record.powered_count > 0;
            let inputs = vp::VehiclePhysicsInputs {
                index: record.vehicle_index,
                powered: driven.then_some(powered.as_slice()),
                magic: (record.has_magic != 0).then_some((record.magic_force, record.magic_torque)),
                bounding_center_after: record.after.bounding_center,
            };
            let b = &record.before;
            let mut state = vp::VehiclePhysicsState {
                position: b.position,
                forward: b.forward,
                up: b.up,
                translational_velocity: b.translational_velocity,
                angular_velocity: b.angular_velocity,
                object_flags: b.object_flags,
                location_leaf: b.location_leaf,
                location_cluster: b.location_cluster,
                collision_force: b.collision_force,
                collision_torque: b.collision_torque,
                stuck_mass_point_flags: b.stuck_mass_point_flags,
            };
            let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) else { return };
            bsp.breakable_surfaces.set(Some(record.breakable_surfaces));
            let (Some(Some(bsp)), Some(Some(clusters))) = (self.bsps.get(&structure_bsp_index), self.clusters.get(&structure_bsp_index)) else {
                return;
            };
            let world = halo_map::collision::World { collision: bsp, clusters, objects: &world_objects, source: None };
            let env = vp::VehiclePhysicsWorld {
                world: &world,
                feature_objects: &feature_objects,
                materials: self.material_physics.as_deref().unwrap_or(&[]),
                others: &others,
            };
            let outcome = vp::physics_update(&physics, &mut state, &inputs, &env);
            if let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) {
                bsp.breakable_surfaces.set(None);
            }
            self.vehicle_physics[1] += 1;
            let a = &record.after;
            let bits = |v: [f32; 3]| v.map(f32::to_bits);
            let same_state = bits(state.position) == bits(a.position)
                && bits(state.forward) == bits(a.forward)
                && bits(state.up) == bits(a.up)
                && bits(state.translational_velocity) == bits(a.translational_velocity)
                && bits(state.angular_velocity) == bits(a.angular_velocity)
                && state.object_flags == a.object_flags
                && state.location_leaf == a.location_leaf
                && state.location_cluster == a.location_cluster
                && bits(state.collision_force) == bits(a.collision_force)
                && bits(state.collision_torque) == bits(a.collision_torque)
                && state.stuck_mass_point_flags == a.stuck_mass_point_flags;
            let point_bytes = |m: &vp::MassPoint| {
                // SAFETY: MassPoint is repr(C) plain old data of 304 bytes
                unsafe { std::mem::transmute::<vp::MassPoint, [u8; 304]>(*m) }
            };
            let first_point = outcome.mass_points.iter().zip(&update.mass_points).position(|(m, g)| point_bytes(m) != *g);
            let same_points = outcome.mass_points.len() == update.mass_points.len() && first_point.is_none();
            if same_state {
                self.vehicle_physics[3] += 1;
            }
            if same_state && same_points {
                let k = &mut self.vehicle_physics;
                k[2] += 1;
                k[7] += driven as u64;
                k[8] += outcome.moved as u64;
                k[9] += (outcome.passes > 1 || !outcome.moved) as u64;
                k[10] += (a.object_flags & vp::object_flags::ON_GROUND != 0) as u64;
                k[11] += (a.object_flags & vp::object_flags::ON_MEDIA != 0) as u64;
                k[12] += (outcome.vehicle_collisions > 0) as u64;
                k[13] += outcome.mass_points.iter().any(|m| m.flags & vp::point_flags::ANTIGRAVING != 0) as u64;
            } else if self.vehicle_physics_shown < 8 {
                self.vehicle_physics_shown += 1;
                println!(
                    "  vehicle physics {:#x} (definition {:#x}) differs (state agrees {same_state}; first differing mass point {first_point:?} of {}):\n    before   {:?}\n    game     {:?}\n    halo_map {:?}\n    outcome passes {} moved {} collisions {}",
                    record.vehicle_index,
                    record.definition_index,
                    outcome.mass_points.len(),
                    record.before,
                    record.after,
                    state,
                    outcome.passes,
                    outcome.moved,
                    outcome.vehicle_collisions
                );
                if let Some(i) = first_point {
                    // SAFETY: as above, read back
                    let game: vp::MassPoint = unsafe { std::mem::transmute::<[u8; 304], vp::MassPoint>(update.mass_points[i]) };
                    println!("    mass point {i}: game {:?}\n                 halo_map {:?}", game, outcome.mass_points[i]);
                }
            }
        }
    }

    /// Loads every animation of a graph for posing.
    fn load_graph_animations(&mut self, graph_index: u32) {
        if !self.unit_pose_graphs.insert(graph_index) {
            return;
        }
        let cache = &self.cache;
        for index in 0..i16::MAX {
            match halo_map::pose::PoseAnimation::load(cache, graph_index, index) {
                Ok(Some(animation)) => {
                    self.pose_animations.insert((graph_index, index), Some(animation));
                }
                _ => break,
            }
        }
    }

    /// Replays an object type's preprocess of its pose (a unit's
    /// `unit_preprocess_node_orientations`, then a vehicle's; a device's;
    /// for the other types and objects without a graph, none) over the
    /// orientations the game's animation and overlays gave it.
    fn check_preprocess(
        &mut self,
        record: &halo_bridge::PoseRecord,
        nodes: &[halo_bridge::PoseNodeRecord],
        model_index: u32,
        object: &halo_map::pose::PoseObjectDefinition,
    ) {
        use halo_map::pose::Orientation;
        use halo_map::unit_pose::{GrenadeThrowLegs, UnitPoseInputs, unit_preprocess_node_orientations};
        let kind = match record.kind {
            0 => 0,
            1 => 1,
            2 => 2,
            7..=9 => 3,
            _ => 4,
        };
        let mut orientations: Vec<Orientation> = nodes.iter().map(|n| Orientation::from_floats(n.animated)).collect();
        let mut unit_outputs = None;
        let mut unit_details = None;
        if let Some(graph_index) = object.animation_graph {
            let cache = &self.cache;
            if record.unit_recorded != 0 && !self.animation_graphs.contains_key(&graph_index) {
                self.animation_graphs.insert(graph_index, halo_map::unit_animation::AnimationGraph::load(cache, graph_index).ok());
            }
            if record.vehicle_recorded != 0 && !self.vehicle_pose_definitions.contains_key(&record.definition_index) {
                let loaded = halo_map::vehicle_pose::VehiclePoseDefinition::load(cache, record.definition_index).ok();
                self.vehicle_pose_definitions.insert(record.definition_index, loaded);
            }
            if record.device_recorded != 0 && !self.device_pose_definitions.contains_key(&record.definition_index) {
                let loaded = halo_map::device_pose::DevicePoseDefinition::load(cache, record.definition_index).ok();
                self.device_pose_definitions.insert(record.definition_index, loaded);
            }
            let Some(Some(model)) = self.pose_models.get(&model_index) else { return };
            let animations = &self.pose_animations;
            let animation = |index: i16| animations.get(&(graph_index, index)).and_then(|a| a.as_ref());
            if record.unit_recorded != 0 {
                let Some(Some(graph)) = self.animation_graphs.get(&graph_index) else { return };
                // (the unit definition's flags, after the object's)
                let Ok(unit_flags) = cache
                    .tag_root(record.definition_index, halo_map::groups::OBJECT)
                    .and_then(|root| root.u32(halo_map::layout::biped_definition::UNIT + halo_map::layout::_unit_definition::FLAGS))
                else {
                    return;
                };
                let inputs = UnitPoseInputs {
                has_no_aiming: unit_flags & (1 << 10) != 0,
                simple_creature: unit_flags & (1 << 11) != 0,
                enhanced: record.unit_enhanced != 0,
                airborne: record.unit_airborne != 0,
                parented: record.unit_parented != 0,
                has_weapon: record.unit_has_weapon != 0,
                player: record.unit_player != 0,
                game_time: record.game_time,
                animation_flags: record.unit_animation_flags,
                aiming_screen_index: record.aiming_screen_index,
                looking_screen_index: record.looking_screen_index,
                override_emotion_animation_index: record.override_emotion_animation_index,
                seat_index: record.seat_index,
                weapon_index: record.weapon_index,
                state: record.state,
                action: record.action,
                base_seat_index: record.base_seat_index,
                emotion_index: record.emotion_index,
                action_animation: (record.action_animation[0], record.action_animation[1]),
                overlay_action_animation: (record.overlay_action_animation[0], record.overlay_action_animation[1]),
                soft_ping_animation: (record.soft_ping_animation[0], record.soft_ping_animation[1]),
                mouth_aperture: record.mouth_aperture,
                seat_acceleration: record.seat_acceleration,
                aiming_vector: record.aiming_vector,
                looking_vector: record.looking_vector,
                orientation_forward: record.orientation_forward,
                orientation_up: record.orientation_up,
                forward: record.forward,
                up: record.up,
                translational_velocity: record.translational_velocity,
            };
                let mut legs = GrenadeThrowLegs {
                    leg_weight: record.grenade_leg_weight,
                    leg_time: record.grenade_leg_time,
                    air_weight: record.grenade_air_weight,
                    air_time: record.grenade_air_time,
                };
                let outputs = unit_preprocess_node_orientations(model, graph, &animation, &inputs, &mut legs, &mut orientations);
                unit_outputs = Some(outputs);
                unit_details = Some((inputs, legs));
            }
            if record.vehicle_recorded != 0 {
                let Some(Some(definition)) = self.vehicle_pose_definitions.get(&record.definition_index) else { return };
                let inputs = halo_map::vehicle_pose::VehiclePoseInputs {
                    turn: record.vehicle_turn,
                    speed: record.vehicle_speed,
                    wheel: record.vehicle_wheel,
                    suspension: record.vehicle_suspension,
                    forward: record.forward,
                    up: record.up,
                    translational_velocity: record.translational_velocity,
                };
                halo_map::vehicle_pose::vehicle_preprocess_node_orientations(definition, &animation, nodes.len(), &inputs, &mut orientations);
            }
            if record.device_recorded != 0 {
                let Some(Some(definition)) = self.device_pose_definitions.get(&record.definition_index) else { return };
                halo_map::device_pose::device_preprocess_node_orientations(
                    definition,
                    &animation,
                    record.device_flags,
                    record.device_position,
                    record.device_power,
                    &mut orientations,
                );
            }
        }

        let bits = |f: [f32; 4]| f.map(f32::to_bits);
        let same_orientations = orientations.iter().zip(nodes).all(|(o, n)| o.to_floats().map(f32::to_bits) == n.preprocessed.map(f32::to_bits));
        let same_outputs = unit_outputs.is_none_or(|outputs| {
            outputs.aiming_with_euler_screen == (record.aiming_with_euler_screen != 0)
                && outputs.looking_with_euler_screen == (record.looking_with_euler_screen != 0)
                && outputs.aiming_screen_bounds.is_none_or(|b| bits(b) == bits(record.aiming_screen_bounds))
                && outputs.looking_screen_bounds.is_none_or(|b| bits(b) == bits(record.looking_screen_bounds))
        });
        let changed = nodes.iter().any(|n| n.animated.map(f32::to_bits) != n.preprocessed.map(f32::to_bits));
        self.preprocess_kinds[kind][0] += 1;
        if same_orientations && same_outputs {
            self.preprocess_kinds[kind][1] += 1;
            self.preprocess_kinds[kind][2] += changed as u64;
            if record.vehicle_recorded != 0 {
                let t = &mut self.vehicle_poses;
                t[0] += (record.vehicle_turn != 0.0) as u64;
                t[1] += (record.vehicle_speed != 0.0) as u64;
                t[2] += (record.vehicle_wheel != 0.0) as u64;
                t[3] += record.vehicle_suspension.iter().any(|&c| c != 0) as u64;
            }
            if record.device_recorded != 0 {
                let t = &mut self.device_poses;
                t[0] += (record.device_position != 0.0) as u64;
                t[1] += (record.device_power != 0.0) as u64;
            }
            if let (Some(outputs), Some((inputs, legs))) = (unit_outputs, &unit_details) {
                let k = &mut self.unit_poses;
                k[0] += 1;
                k[1] += 1;
                k[2] += (inputs.action_animation.0 != -1) as u64;
                k[3] += (inputs.overlay_action_animation.0 != -1) as u64;
                k[4] += (inputs.soft_ping_animation.0 != -1) as u64;
                k[5] += (inputs.emotion_index != -1 || inputs.mouth_aperture > 0.0 || inputs.animation_flags & 2 != 0) as u64;
                k[6] += outputs.aiming_with_euler_screen as u64;
                k[7] += outputs.looking_with_euler_screen as u64;
                if inputs.state == 33 && inputs.enhanced {
                    k[8] += 1;
                    let v = inputs.translational_velocity;
                    let t = &mut self.unit_pose_throws;
                    t[0] += (v[0] != 0.0 || v[1] != 0.0) as u64;
                    t[1] += inputs.airborne as u64;
                    t[2] += (inputs.base_seat_index == 3) as u64;
                    t[3] += inputs.parented as u64;
                    t[4] += (*legs != GrenadeThrowLegs {
                        leg_weight: record.grenade_leg_weight,
                        leg_time: record.grenade_leg_time,
                        air_weight: record.grenade_air_weight,
                        air_time: record.grenade_air_time,
                    }) as u64;
                }
            }
        } else if self.unit_poses_shown < 8 {
            self.unit_poses_shown += 1;
            if unit_outputs.is_some() {
                self.unit_poses[0] += 1;
            }
            let first = orientations.iter().zip(nodes).position(|(o, n)| o.to_floats().map(f32::to_bits) != n.preprocessed.map(f32::to_bits));
            println!(
                "  preprocess of {:#x} (type {}, definition {:#x}) differs at node {first:?} of {} (unit outputs agree {same_outputs}: halo_map {:?}, game aiming {} {:?} looking {} {:?}; vehicle {} turn {} speed {} wheel {} suspension {:?}; device {} flags {:#x} position {} power {}):\n    unit     {:?}\n    animated {:?}\n    game     {:?}\n    halo_map {:?}",
                record.object_index,
                record.kind,
                record.definition_index,
                nodes.len(),
                unit_outputs,
                record.aiming_with_euler_screen,
                record.aiming_screen_bounds,
                record.looking_with_euler_screen,
                record.looking_screen_bounds,
                record.vehicle_recorded,
                record.vehicle_turn,
                record.vehicle_speed,
                record.vehicle_wheel,
                record.vehicle_suspension,
                record.device_recorded,
                record.device_flags,
                record.device_position,
                record.device_power,
                unit_details.as_ref().map(|(inputs, _)| inputs),
                first.map(|i| nodes[i].animated),
                first.map(|i| nodes[i].preprocessed),
                first.map(|i| orientations[i].to_floats())
            );
        } else if unit_outputs.is_some() {
            self.unit_poses[0] += 1;
        }
    }

    fn report(&self) {
        let [recorded, compared, agreed, elsewhere, missing, controls, with_physics, coasting, riding, righting, throttled] = self.vehicle_updates;
        println!(
            "vehicles: {} of {} vehicle updates replayed agree with the game byte for byte, with the controls handed to their physics ({} the controls alone; {} with physics, {} coasting, {} riding a parent, {} righting, {} with a throttle; by type tank/jeep/boat/plane/scout/fighter/turret {:?}); of {} recorded, left out: {} missing their physics record or objects, {} elsewhere",
            agreed, compared, controls, with_physics, coasting, riding, righting, throttled, self.vehicle_update_types, recorded, missing, elsewhere
        );
        let [recorded, compared, agreed, state_agreed, old, lost, elsewhere, driven, moved, frozen, ground, water, collided, antigrav] =
            self.vehicle_physics;
        println!(
            "vehicles: {} of {} physics updates replayed agree with the game byte for byte, state and mass points ({} the state alone; {} driven, {} moved, {} frozen against the world, {} on the ground, {} in water, {} colliding with another vehicle, {} antigraving); of {} recorded ({} with the old physics), left out: {} whose objects the rings lost, {} elsewhere",
            agreed, compared, state_agreed, driven, moved, frozen, ground, water, collided, antigrav, recorded, old, lost, elsewhere
        );
        let [compared, agreed, vehicles, others, structure] = self.vehicle_rays;
        println!(
            "collision: {} of {} rays at vehicles tested with their physics shapes agree with the game bit for bit ({} hit a vehicle's mass points, {} another object, {} the structure)",
            agreed, compared, vehicles, others, structure
        );
        let [compared, agreed, armed, unarmed, damaged, under_way] = self.melees;
        println!(
            "bipeds: {} of {} players' melee steps replayed agree with the game ({} melees started with a weapon, {} without; {} hits landed; {} steps under way)",
            agreed, compared, armed, unarmed, damaged, under_way
        );
        let [compared, agreed, actions, overlay_actions, soft_pings, seat_overlays, aiming, looking, throws] = self.unit_poses;
        println!(
            "poses: each type's preprocess (biped, vehicle, weapon, device, other: compared, agreed, of those changing the pose) {:?}; vehicles turning, moving, wheels turned, suspensions compressed {:?}; devices with a position, a power {:?}",
            self.preprocess_kinds, self.vehicle_poses, self.device_poses
        );
        println!(
            "poses: {} of {} units' preprocessed poses agree with the game bit for bit ({} with an action, {} an overlay action, {} a soft ping, {} an emotion, mouth or acceleration overlay; {} aiming, {} looking; {} throwing a grenade: {:?} moving, airborne, crouched, riding, easing)",
            agreed, compared, actions, overlay_actions, soft_pings, seat_overlays, aiming, looking, throws, self.unit_pose_throws
        );
        let [compared, agreed, overlays, compressed, model_pose] = self.pose_animated;
        let [m_compared, m_agreed, interpolating, parented] = self.pose_matrices;
        println!(
            "poses: {} of {} sampled poses' orientations agree with the game bit for bit ({} from compressed animations, {} the model's pose, {} with object overlays); {} of {} poses' node matrices agree ({} interpolating, {} hanging from a parent); by type {:?}; {} of {} lost from the ring",
            agreed, compared, compressed, model_pose, overlays, m_agreed, m_compared, interpolating, parented, self.pose_types, self.poses_incomplete, self.poses
        );
        let [entered, seats, restarted, with_effects, leaps] = self.animation_kinds;
        println!(
            "bipeds: {} of {} unit animation updates replayed agree with the game ({} states entered, {} seat or weapon changes, {} animations restarted, {} with side effects (bits {:#x}), {} leaps)",
            self.animation_agreed, self.animation_updates, entered, seats, restarted, with_effects, self.animation_side_effects, leaps
        );
        let [dead, airborne, landing, flying_turns, moving_turns, standing_turns, tumbling] = self.update_kinds;
        println!(
            "bipeds: {} of {} biped updates (to the animation) replayed agree with the game byte for byte ({} dead, {} airborne, {} landing; {} flying turns, {} moving turns, {} standing turns asked for, {} tumbling; {} erased by the game for falling outside the world); left out: {} whose movement record was lost, {} elsewhere, of {}",
            self.update_agreed,
            self.update_replayed,
            dead,
            airborne,
            landing,
            flying_turns,
            moving_turns,
            standing_turns,
            tumbling,
            self.update_erased,
            self.update_missing,
            self.update_elsewhere,
            self.update_updates
        );
        let [player, animated, flying, turned, jumps, landings, aborted, crouching] = self.moving_kinds;
        println!(
            "bipeds: {} of {} movement updates replayed agree with the game byte for byte ({} of {} physics inputs agree; {} player physics, {} driven by animation, {} flying; {} turned by the animation, {} jumps, {} landings, {} aborted animations, {} crouching or standing; {} erased by the game for falling outside the world); left out: {} whose physics record was lost, {} elsewhere, of {}",
            self.moving_agreed,
            self.moving_replayed,
            self.moving_physics_agreed,
            self.moving_physics_compared,
            player,
            animated,
            flying,
            turned,
            jumps,
            landings,
            aborted,
            crouching,
            self.moving_erased,
            self.moving_missing_physics,
            self.moving_elsewhere,
            self.moving_updates
        );
        println!(
            "bipeds: of the agreeing updates, bumped by type (biped, vehicle, ..., scenery, machine, control) {:?}; {} rode an elevator",
            self.bipeds_bumped_kinds, self.bipeds_on_elevators
        );
        let [flying, airborne, walking, supported, stuck, cannot_stand] = self.biped_kinds;
        println!(
            "bipeds: {} of {} physics updates replayed agree with the game byte for byte ({} bipeds; {} walking, {} airborne, {} flying; {} ended on the ground, {} held to a ledge, {} could not stand; {} bumped an object or rode an elevator; {} objects visited); of {} recorded, left out: {} whose objects the rings lost, {} elsewhere",
            self.bipeds_agreed,
            self.bipeds_replayed,
            self.bipeds_seen.len(),
            walking,
            airborne,
            flying,
            supported,
            stuck,
            cannot_stand,
            self.bipeds_touched_objects,
            self.biped_object_features,
            self.biped_updates,
            self.bipeds_incomplete,
            self.bipeds_elsewhere
        );
        let [other_bsp, incomplete, missing_input, many_collisions] = self.projectile_skipped;
        let [touched, damaged, attached, overpenetrated, steered, combining] = self.projectile_object_kinds;
        println!(
            "projectiles: {} of {} updates replayed agree with the game bit for bit ({} collisions; {} projectiles; ended none/detonate/disappear {:?}; {} touching an object, {} damaging one, {} sticking to one, {} ignoring one, {} steering, {} super-combining); of {} recorded, left out: {} whose objects the rings lost, {} missing an input, {} with over 4 collisions, {} elsewhere",
            self.projectiles_agreed,
            self.projectiles_replayed,
            self.projectile_collisions,
            self.projectiles_seen.len(),
            self.projectile_actions,
            touched,
            damaged,
            attached,
            overpenetrated,
            steered,
            combining,
            self.projectile_updates,
            incomplete,
            missing_input,
            many_collisions,
            other_bsp
        );
        println!(
            "physics: {} of {} points agree with the game bit for bit after {} steps of point physics ({} collisions; {} points hit water; definition flags {:#x})",
            self.points_agreed, self.points, halo_bridge::POINT_PHYSICS_STEPS, self.point_collisions, self.points_in_water, self.point_definition_flags
        );
        println!(
            "collision: {} of {} whole ray tests (structure and objects, the broad phase in Rust) agree with the game bit for bit; {} of {} that hit an object",
            self.whole_agreed, self.whole_tests, self.object_hits_agreed, self.object_hits
        );
        println!(
            "collision: {} of {} swept spheres agree with the game bit for bit ({} touched the structure)",
            self.pills_agreed, self.pills, self.pill_hits
        );
        println!(
            "collision: {} of {} spheres agree with the game ({} surfaces, edges and vertices touched)",
            self.spheres_agreed, self.spheres, self.sphere_features
        );
        println!(
            "collision: {} of {} rays agree with the game ({} hit the structure); largest t difference {}",
            self.agreed, self.rays, self.hits, self.largest_t_difference
        );
    }
}
