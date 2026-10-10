//! Prints what a running OpenCE shares through its Bevy bridge, once a second.
//!
//! ```text
//! bridge_probe [seconds] [walk] [listen] [pad] [latency] [fire] [<map file>]
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
            let records = bridge.read_projectile_records();
            if !records.is_empty() {
                let directions = bridge.random_directions();
                check.check_projectiles(snapshot.structure_bsp_index, &records, &directions);
            }
            let bipeds = bridge.read_biped_records();
            if !bipeds.is_empty() {
                check.check_bipeds(snapshot.structure_bsp_index, &bipeds);
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
    bipeds_elsewhere: u64,
    bipeds_seen: std::collections::HashSet<u32>,
    biped_kinds: [u64; 6],
    bipeds_shown: u32,
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
            bipeds_elsewhere: 0,
            bipeds_seen: Default::default(),
            biped_kinds: [0; 6],
            bipeds_shown: 0,
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
            });
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
                let world = halo_map::collision::World { collision: bsp, clusters, objects: world_objects };
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
                let world = halo_map::collision::World { collision: bsp, clusters, objects: &world_objects };
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
    fn check_projectiles(&mut self, structure_bsp_index: i32, records: &[halo_bridge::ProjectileRecord], directions: &[[f32; 3]]) {
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
        let world = halo_map::collision::World { collision: bsp, clusters, objects: &[] };
        for record in records {
            self.projectile_updates += 1;
            self.projectiles_seen.insert(record.projectile_index);
            let skip = if record.structure_bsp_index != structure_bsp_index {
                Some(0)
            } else if record.touched_object != 0 {
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
            let mut random = halo_map::projectile::Random { seed: record.random_seed, directions };
            let seeds = &record.collision_seeds[..record.collision_count as usize];
            let Some(summary) = halo_map::projectile::projectile_update(&world, definition, &mut state, &mut random, seeds) else {
                self.projectile_skipped[2] += 1;
                continue;
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
            } else if self.projectiles_shown < 10 {
                self.projectiles_shown += 1;
                println!(
                    "  projectile {:#x} (definition {:#x}) differs after {} collisions (halo_map {}):\n    before {:?}\n    game   {:?}\n    halo_map {:?}",
                    record.projectile_index, record.definition_index, record.collision_count, summary.collisions, record.before, record.after, state
                );
            }
        }
    }

    /// Replays the biped physics updates the game recorded with halo_map's
    /// `biped_update_physics`, from the physics each began with and the
    /// breakable surfaces standing then, and compares what they leave byte
    /// for byte. Updates that touched an object (one bumped, or an
    /// elevator) are left out: objects' collisions aren't ported.
    fn check_bipeds(&mut self, structure_bsp_index: i32, records: &[halo_bridge::BipedRecord]) {
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
        let Some(Some(bsp)) = self.bsps.get_mut(&structure_bsp_index) else { return };
        for record in records {
            self.biped_updates += 1;
            self.bipeds_seen.insert(record.biped_index);
            if record.structure_bsp_index != structure_bsp_index {
                self.bipeds_elsewhere += 1;
                continue;
            }
            if record.after.bumped_object_index != -1 || record.after.elevator_object_index != -1 {
                self.bipeds_touched_objects += 1;
                continue;
            }
            bsp.breakable_surfaces = Some(record.breakable_surfaces);
            let mut physics = record.before;
            let stand = (record.stand_test != 0).then_some(halo_map::biped::StandTest {
                collision_height_standing: record.stand_height,
                collision_radius: record.stand_radius,
            });
            halo_map::biped::biped_update_physics(bsp, &mut physics, stand);
            bsp.breakable_surfaces = None;
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
            } else if self.bipeds_shown < 6 {
                self.bipeds_shown += 1;
                println!(
                    "  biped {:#x} differs:\n    before   {:?}\n    game     {:?}\n    halo_map {:?}",
                    record.biped_index, record.before, record.after, physics
                );
            }
        }
    }

    fn report(&self) {
        let [flying, airborne, walking, supported, stuck, cannot_stand] = self.biped_kinds;
        println!(
            "bipeds: {} of {} physics updates replayed agree with the game byte for byte ({} bipeds; {} walking, {} airborne, {} flying; {} ended on the ground, {} held to a ledge, {} could not stand); of {} recorded, left out: {} touched an object, {} elsewhere",
            self.bipeds_agreed,
            self.bipeds_replayed,
            self.bipeds_seen.len(),
            walking,
            airborne,
            flying,
            supported,
            stuck,
            cannot_stand,
            self.biped_updates,
            self.bipeds_touched_objects,
            self.bipeds_elsewhere
        );
        let [other_bsp, touched_object, guided, many_collisions] = self.projectile_skipped;
        println!(
            "projectiles: {} of {} updates replayed agree with the game bit for bit ({} collisions; {} projectiles; ended none/detonate/disappear {:?}); of {} recorded, left out: {} hit an object, {} guided, {} with over 4 collisions, {} elsewhere",
            self.projectiles_agreed,
            self.projectiles_replayed,
            self.projectile_collisions,
            self.projectiles_seen.len(),
            self.projectile_actions,
            self.projectile_updates,
            touched_object,
            guided,
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
