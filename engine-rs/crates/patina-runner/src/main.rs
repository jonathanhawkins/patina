//! Patina headless runner — loads and executes a `.tscn` scene file.
//!
//! This binary parses a Godot `.tscn` file, instances the scene into a
//! [`SceneTree`], runs lifecycle callbacks and a configurable number of
//! frames, then dumps the final tree state as JSON to stdout.
//!
//! # Usage
//!
//! ```text
//! patina-runner <scene.tscn> [--frames N] [--delta D]
//! ```

#![warn(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process;

use gdscene::node::NodeId;
use gdscene::scene_tree::SceneTree;
use gdscene::{LifecycleManager, MainLoop, PackedScene, add_packed_scene_to_tree};
use gdvariant::serialize::to_json;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

/// Parsed command-line arguments.
struct Args {
    /// Path to the `.tscn` file.
    scene_path: String,
    /// Number of frames to run (default 10).
    frames: u64,
    /// Delta time per frame in seconds (default 1/60).
    delta: f64,
}

/// Parses command-line arguments manually (no extra dependency).
fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        return Err(format!(
            "Usage: {} <scene.tscn> [--frames N] [--delta D]",
            args[0]
        ));
    }

    let scene_path = args[1].clone();
    let mut frames: u64 = 10;
    let mut delta: f64 = 1.0 / 60.0;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--frames" => {
                i += 1;
                frames = args
                    .get(i)
                    .ok_or("--frames requires a value")?
                    .parse()
                    .map_err(|e| format!("invalid --frames value: {e}"))?;
            }
            "--delta" => {
                i += 1;
                delta = args
                    .get(i)
                    .ok_or("--delta requires a value")?
                    .parse()
                    .map_err(|e| format!("invalid --delta value: {e}"))?;
            }
            other => {
                return Err(format!("unknown argument: {other}"));
            }
        }
        i += 1;
    }

    Ok(Args {
        scene_path,
        frames,
        delta,
    })
}

// ---------------------------------------------------------------------------
// Tree dump
// ---------------------------------------------------------------------------

/// Walks the scene tree and serializes each node into a JSON value.
///
/// The output is a nested structure mirroring the node hierarchy, with each
/// node carrying its name, class, path, properties, and notification log.
pub fn dump_tree_json(tree: &SceneTree) -> Value {
    dump_node_json(tree, tree.root_id())
}

/// Recursively serializes a single node and its children.
fn dump_node_json(tree: &SceneTree, id: NodeId) -> Value {
    let node = match tree.get_node(id) {
        Some(n) => n,
        None => return json!(null),
    };

    let path = tree.node_path(id).unwrap_or_default();

    // Collect properties in sorted order for deterministic output.
    let mut props = serde_json::Map::new();
    let sorted_props: BTreeMap<&String, &gdvariant::Variant> = node.properties().collect();
    for (key, value) in sorted_props {
        props.insert(key.clone(), to_json(value));
    }

    // Notification log as human-readable strings.
    let notifications: Vec<String> = node
        .notification_log()
        .iter()
        .map(|n| format!("{n}"))
        .collect();

    // Recursively serialize children.
    let children: Vec<Value> = node
        .children()
        .iter()
        .map(|&child_id| dump_node_json(tree, child_id))
        .collect();

    json!({
        "name": node.name(),
        "class": node.class_name(),
        "path": path,
        "properties": props,
        "notifications": notifications,
        "children": children,
    })
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    // Initialize tracing to stderr so stdout stays clean for JSON.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    tracing::info!(
        scene = %args.scene_path,
        frames = args.frames,
        delta = args.delta,
        "loading scene"
    );

    // Read and parse the .tscn file.
    let source = match fs::read_to_string(&args.scene_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {e}", args.scene_path);
            process::exit(1);
        }
    };

    let packed_scene = match PackedScene::from_tscn(&source) {
        Ok(ps) => ps,
        Err(e) => {
            eprintln!("Error parsing '{}': {e}", args.scene_path);
            process::exit(1);
        }
    };

    // Create the scene tree and add the instanced scene.
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    let scene_root_id = match add_packed_scene_to_tree(&mut tree, root_id, &packed_scene) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error instancing scene: {e}", );
            process::exit(1);
        }
    };

    // Run lifecycle: enter_tree + ready.
    LifecycleManager::enter_tree(&mut tree, scene_root_id);

    tracing::info!(node_count = tree.node_count(), "scene instanced");

    // Create the main loop and run frames.
    let mut main_loop = MainLoop::new(tree);
    main_loop.run_frames(args.frames, args.delta);

    tracing::info!(
        frame_count = main_loop.frame_count(),
        physics_time = main_loop.physics_time(),
        process_time = main_loop.process_time(),
        "simulation complete"
    );

    // Dump final state as JSON to stdout.
    let output = json!({
        "scene_file": args.scene_path,
        "frame_count": main_loop.frame_count(),
        "physics_time": main_loop.physics_time(),
        "process_time": main_loop.process_time(),
        "tree": dump_tree_json(main_loop.tree()),
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("JSON serialization failed")
    );
}
