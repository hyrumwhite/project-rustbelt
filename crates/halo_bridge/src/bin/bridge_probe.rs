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

/// The game's ray tests against halo_map's collision code: how many agree
/// (hit or miss, the surface, and t to within a millionth).
struct CollisionCheck {
    cache: halo_map::CacheFile,
    bsps: std::collections::HashMap<i32, Option<halo_map::collision::CollisionBsp>>,
    rays: u64,
    agreed: u64,
    hits: u64,
    largest_t_difference: f32,
    shown: u32,
}

impl CollisionCheck {
    fn new(path: &str) -> Self {
        let cache = halo_map::CacheFile::open(std::path::Path::new(path)).expect("the map opens");
        CollisionCheck {
            cache,
            bsps: Default::default(),
            rays: 0,
            agreed: 0,
            hits: 0,
            largest_t_difference: 0.0,
            shown: 0,
        }
    }

    fn check(&mut self, snapshot: &halo_bridge::Snapshot) {
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
            if agree {
                self.agreed += 1;
            } else if self.shown < 5 {
                self.shown += 1;
                println!(
                    "  ray differs: game hit {} t {} surface {}; halo_map {:?}",
                    test.hit, test.t, test.surface_index, ours.map(|h| (h.t, h.surface))
                );
            }
        }
    }

    fn report(&self) {
        println!(
            "collision: {} of {} rays agree with the game ({} hit the structure); largest t difference {}",
            self.agreed, self.rays, self.hits, self.largest_t_difference
        );
    }
}
