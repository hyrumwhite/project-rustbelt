//! How steadily the first-person weapon and hands sit in the camera's view.
//!
//! Reads a running game's bridge (`HALO_BEVY_BRIDGE_NAME`, default
//! HaloBevyBridge) for the given seconds. For each frame, the models the game
//! drew by themselves (the first-person weapon and hands) are taken into the
//! camera's space. The weapon is held still in that space, but for its
//! animation, so a pose from another frame than the camera's shows as motion
//! there whenever the camera moves.
//!
//! `cargo run --release -p halo_bridge --example first_person_drift -- 30`

use halo_bridge::{Bridge, Camera, Node, OBJECT_MODEL};
use std::time::{Duration, Instant};

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A point in the camera's space: along its forward, left and up.
fn camera_space(camera: &Camera, point: [f32; 3]) -> [f32; 3] {
    let offset = sub(point, camera.position);
    let left = cross(camera.up, camera.forward);
    [dot(offset, camera.forward), dot(offset, left), dot(offset, camera.up)]
}

fn percentile(values: &mut [f32], fraction: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    values[((values.len() - 1) as f32 * fraction) as usize]
}

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(30);
    let name = std::env::var("HALO_BEVY_BRIDGE_NAME").unwrap_or_else(|_| "HaloBevyBridge".into());
    let start = Instant::now();
    let mut bridge = loop {
        match Bridge::open_named(&name) {
            Ok(bridge) => break bridge,
            Err(_) if start.elapsed() < Duration::from_secs(60) => std::thread::sleep(Duration::from_millis(200)),
            Err(error) => {
                eprintln!("no bridge {name}: {error}");
                std::process::exit(1);
            }
        }
    };
    // (per frame: the weapon's root in the camera's space, and how far the
    // camera moved and turned since the frame before)
    let mut previous: Option<([f32; 3], Camera, u32)> = None;
    let mut steps = Vec::new();
    let mut moving_steps = Vec::new();
    let mut frames = 0;
    // (the models seen: their tag and node count, and in how many frames)
    let mut models = std::collections::BTreeMap::<(u32, u16), u32>::new();
    while start.elapsed() < Duration::from_secs(seconds) {
        let Some(snapshot) = bridge.read() else {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        let (Some(camera), Some(weapon)) = (snapshot.camera, snapshot.objects.iter().find(|o| o.flags & OBJECT_MODEL != 0)) else {
            previous = None;
            continue;
        };
        for model in snapshot.objects.iter().filter(|o| o.flags & OBJECT_MODEL != 0) {
            *models.entry((model.definition_index, model.node_count)).or_default() += 1;
            if frames == 600 {
                println!("vertical field of view {:.2} degrees", camera.vertical_field_of_view.to_degrees());
                let distances: Vec<String> = snapshot
                    .object_nodes(model)
                    .iter()
                    .map(|n| format!("{:.3}", camera_space(&camera, n.position)[0]))
                    .collect();
                println!("model {:08x} nodes' distances ahead of the camera: {}", model.definition_index, distances.join(" "));
            }
        }
        let nodes: &[Node] = snapshot.object_nodes(weapon);
        let Some(root) = nodes.first() else { continue };
        let here = camera_space(&camera, root.position);
        frames += 1;
        if let Some((before, before_camera, before_frame)) = previous
            && snapshot.frame_index == before_frame + 1
        {
            let step = sub(here, before);
            let step = dot(step, step).sqrt();
            let moved = sub(camera.position, before_camera.position);
            let moved = dot(moved, moved).sqrt();
            let turned = 1.0 - dot(camera.forward, before_camera.forward).clamp(-1.0, 1.0);
            steps.push(step);
            if moved > 0.002 || turned > 1e-6 {
                moving_steps.push(step);
            }
        }
        previous = Some((here, camera, snapshot.frame_index));
    }
    let moving = moving_steps.len();
    println!(
        "{frames} frames with a first-person weapon; its root's step in the camera's space (world units) per frame: median {:.5}, 99th percentile {:.5}, largest {:.5}; while the camera moved ({moving} frames): median {:.5}, 99th percentile {:.5}",
        percentile(&mut steps.clone(), 0.5),
        percentile(&mut steps.clone(), 0.99),
        percentile(&mut steps, 1.0),
        percentile(&mut moving_steps.clone(), 0.5),
        percentile(&mut moving_steps, 0.99),
    );
    for ((definition_index, node_count), seen) in models {
        println!("model tag {definition_index:08x}: {node_count} nodes, in {seen} frames");
    }
}
