//! Reports objects' nodes that are not rigid: a scale other than 1, or axes
//! not of unit length (a pose that shrinks or stretches the model).
//!
//! Reads a running game's bridge (`HALO_BEVY_BRIDGE_NAME`, default
//! HaloBevyBridge) for the given seconds; prints the first few such nodes
//! and a count per object definition.
//!
//! `cargo run --release -p halo_bridge --example node_scales -- 30`

use halo_bridge::Bridge;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
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
    // (per object definition: nodes seen, and those not rigid)
    let mut counts: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    let mut printed = 0;
    let mut frames = 0u64;
    while start.elapsed() < Duration::from_secs(seconds) {
        let Some(snapshot) = bridge.read() else {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        frames += 1;
        for object in &snapshot.objects {
            let entry = counts.entry(object.definition_index).or_default();
            for (index, node) in snapshot.object_nodes(object).iter().enumerate() {
                entry.0 += 1;
                let axes = [length(node.forward), length(node.left), length(node.up)];
                let off = (node.scale - 1.0).abs() > 0.01 || axes.iter().any(|a| (a - 1.0).abs() > 0.01);
                if off {
                    entry.1 += 1;
                    if printed < 20 {
                        printed += 1;
                        println!(
                            "tick {} object {:08x} (definition {:08x}, type {}) node {index}: scale {:.4}, axes {:.4} {:.4} {:.4}",
                            snapshot.game_tick, object.object_index, object.definition_index, object.kind, node.scale, axes[0], axes[1], axes[2]
                        );
                    }
                }
            }
        }
    }
    println!("{frames} frames");
    for (definition, (nodes, off)) in counts.iter().filter(|(_, (_, off))| *off > 0) {
        println!("definition {definition:08x}: {off} of {nodes} nodes not rigid");
    }
}
