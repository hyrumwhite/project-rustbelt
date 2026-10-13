//! Whether the game sends objects' poses blended between its ticks.
//!
//! Reads a running game's bridge (`HALO_BEVY_BRIDGE_NAME`, default
//! HaloBevyBridge) for the given seconds. For each biped that moves, it counts
//! the frames in which its nodes moved since the frame before. The game ticks
//! at 30 Hz and draws faster; poses blended between ticks change every frame,
//! poses as of the last tick only on frames that follow a tick (stop motion).
//! It looks at the nodes' positions relative to the root node, the pose
//! itself rather than where the object is.
//!
//! `cargo run --release -p halo_bridge --example pose_steps -- 30`

use halo_bridge::{Bridge, OBJECT_FIRST_PERSON};
use std::collections::HashMap;
use std::time::{Duration, Instant};

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
    // (per object: its root node's position the frame before, that frame's
    // index and game tick, and whether it moved when the tick last changed)
    let mut previous: HashMap<u32, ([f32; 3], u32, u32, bool)> = HashMap::new();
    // frames within one tick, of bipeds that moved when the tick last
    // changed: in all, and those in which they moved
    let (mut same_tick, mut same_tick_moved, mut crossings) = (0u64, 0u64, 0u64);
    let mut frame_count = 0u64;
    let mut ticks_seen = std::collections::BTreeSet::new();
    while start.elapsed() < Duration::from_secs(seconds) {
        let Some(snapshot) = bridge.read() else {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        frame_count += 1;
        ticks_seen.insert(snapshot.game_tick);
        for object in snapshot.objects.iter().filter(|o| o.kind == 0 && o.flags & OBJECT_FIRST_PERSON == 0) {
            let nodes = snapshot.object_nodes(object);
            let Some(root) = nodes.first() else { continue };
            // (the nodes' offsets from the root, summed: changes when the
            // limbs move)
            let mut here = [0.0f32; 3];
            for node in nodes.iter().skip(1) {
                for axis in 0..3 {
                    here[axis] += (node.position[axis] - root.position[axis]).abs();
                }
            }
            let mut moving = false;
            if let Some(&(before, frame, tick, was_moving)) = previous.get(&object.object_index)
                && frame + 1 == snapshot.frame_index
            {
                let moved = here != before;
                if tick != snapshot.game_tick {
                    moving = moved;
                    crossings += u64::from(moved);
                } else {
                    moving = was_moving;
                    if was_moving {
                        same_tick += 1;
                        same_tick_moved += u64::from(moved);
                    }
                }
            }
            previous.insert(object.object_index, (here, snapshot.frame_index, snapshot.game_tick, moving));
        }
    }
    let elapsed = start.elapsed().as_secs_f32();
    println!(
        "{frame_count} frames ({:.0} a second) over {} ticks; moving bipeds: {crossings} tick changes with motion, {same_tick} frames within a tick of which {same_tick_moved} moved (blended poses move in nearly all; stop motion in none)",
        frame_count as f32 / elapsed,
        ticks_seen.len()
    );
}
