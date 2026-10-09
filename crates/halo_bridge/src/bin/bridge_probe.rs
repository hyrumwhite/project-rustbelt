//! Prints what a running OpenCE shares through its Bevy bridge, once a second.
//!
//! ```text
//! bridge_probe [seconds] [walk] [listen] [pad] [latency] [fire]
//! ```
//!
//! `walk` also forwards input: W held (and the mouse turning slowly) for the
//! second half, to check the game takes input through the bridge. `listen`
//! moves the game's listener to a point of its own for the second half (the
//! game logs the change). `pad` forwards a gamepad, its left stick pushed
//! forward for the second half. `latency` turns the view with the mouse twice
//! a second and reports how long the game's shared camera takes to follow.
//! `fire` holds the left mouse button for the second half.
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
}
