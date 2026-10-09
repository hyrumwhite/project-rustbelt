//! Prints what a running OpenCE shares through its Bevy bridge, once a second.
//!
//! ```text
//! bridge_probe [seconds] [walk] [listen]
//! ```
//!
//! `walk` also forwards input: W held (and the mouse turning slowly) for the
//! second half, to check the game takes input through the bridge. `listen`
//! moves the game's listener to a point of its own for the second half (the
//! game logs the change). `HALO_BEVY_BRIDGE_NAME` names the bridge.

use halo_bridge::{Bridge, OBJECT_FIRST_PERSON, OBJECT_HIDDEN};
use std::time::{Duration, Instant};

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let walk = std::env::args().any(|a| a == "walk");
    // (the game hears from a point above the start for the second half)
    let listen = std::env::args().any(|a| a == "listen");
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
        if let Some(snapshot) = bridge.read() {
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
}
