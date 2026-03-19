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
use std::path::{Path, PathBuf};
use std::process;

use gdscene::node::NodeId;
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{add_packed_scene_to_tree, LifecycleManager, MainLoop, PackedScene};
use gdvariant::serialize::to_json;
use gdvariant::Variant;
use serde_json::{json, Value};

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
    /// Whether to emit a per-frame trace of tree state.
    trace_frames: bool,
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
    let mut trace_frames = false;

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
            "--trace-frames" => {
                trace_frames = true;
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
        trace_frames,
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

    // Collect script variables if a script is attached.
    let mut script_vars = serde_json::Map::new();
    if let Some(script) = tree.get_script(id) {
        for prop_info in script.list_properties() {
            script_vars.insert(prop_info.name.clone(), to_json(&prop_info.default_value));
        }
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
        "script_vars": script_vars,
        "notifications": notifications,
        "children": children,
    })
}

// ---------------------------------------------------------------------------
// Script attachment
// ---------------------------------------------------------------------------

/// Finds nodes with `_script_path` property, resolves `res://` to the
/// project directory, loads the `.gd` source, and attaches a
/// [`GDScriptNodeInstance`] to each node in the scene tree.
fn attach_scripts(tree: &mut SceneTree, project_dir: &Path) {
    // Collect (node_id, script_path) pairs first to avoid borrow issues.
    let mut scripts_to_load: Vec<(NodeId, PathBuf)> = Vec::new();

    for node_id in tree.all_nodes_in_tree_order() {
        if let Some(node) = tree.get_node(node_id) {
            if let Variant::String(res_path) = node.get_property("_script_path") {
                let abs_path = resolve_res_path(project_dir, &res_path);
                scripts_to_load.push((node_id, abs_path));
            }
        }
    }

    for (node_id, path) in scripts_to_load {
        match fs::read_to_string(&path) {
            Ok(source) => match GDScriptNodeInstance::from_source(&source, node_id) {
                Ok(instance) => {
                    tree.attach_script(node_id, Box::new(instance));
                    tracing::info!(
                        path = %path.display(),
                        "attached GDScript to node"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = format!("{e:?}"),
                        "failed to parse GDScript"
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to read GDScript file"
                );
            }
        }
    }
}

/// Resolves a `res://` path to an absolute filesystem path.
fn resolve_res_path(project_dir: &Path, res_path: &str) -> PathBuf {
    let relative = res_path.strip_prefix("res://").unwrap_or(res_path);
    project_dir.join(relative)
}

fn run_main_loop(
    main_loop: &mut MainLoop,
    frames: u64,
    delta: f64,
    trace_frames: bool,
) -> Vec<Value> {
    let mut frame_trace = Vec::new();
    for _ in 0..frames {
        main_loop.step(delta);
        if trace_frames {
            frame_trace.push(json!({
                "frame": main_loop.frame_count(),
                "tree": dump_tree_json(main_loop.tree()),
            }));
        }
    }
    frame_trace
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
            eprintln!("Error instancing scene: {e}",);
            process::exit(1);
        }
    };

    // Resolve and attach GDScript files to nodes that have _script_path.
    let project_dir = Path::new(&args.scene_path)
        .parent()
        .unwrap_or(Path::new("."));
    attach_scripts(&mut tree, project_dir);

    // Run lifecycle: enter_tree + ready.
    LifecycleManager::enter_tree(&mut tree, scene_root_id);

    tracing::info!(node_count = tree.node_count(), "scene instanced");

    // Create the main loop and run frames.
    let mut main_loop = MainLoop::new(tree);
    let frame_trace = run_main_loop(&mut main_loop, args.frames, args.delta, args.trace_frames);

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
    let output = if args.trace_frames {
        let mut output = output;
        output["frame_trace"] = Value::Array(frame_trace);
        output
    } else {
        output
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("JSON serialization failed")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    /// dump_tree_json includes script_vars for nodes with scripts.
    #[test]
    fn dump_tree_json_includes_script_vars() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
func _ready():
    self.speed = 300
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        gdscene::LifecycleManager::enter_tree(&mut tree, child_id);

        let output = dump_tree_json(&tree);
        // Navigate to the child node
        let children = output["children"].as_array().unwrap();
        let player = &children[0];

        // script_vars should contain speed=300
        let script_vars = &player["script_vars"];
        assert_eq!(script_vars["speed"]["value"], json!(300));
    }

    /// dump_tree_json has empty script_vars for nodes without scripts.
    #[test]
    fn dump_tree_json_no_script_empty_script_vars() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Plain", "Node2D");
        let _child_id = tree.add_child(root, child).unwrap();

        let output = dump_tree_json(&tree);
        let children = output["children"].as_array().unwrap();
        let plain = &children[0];

        // script_vars should be an empty object
        assert_eq!(plain["script_vars"], json!({}));
    }

    /// Script vars are also synced to node properties in dump output.
    #[test]
    fn dump_tree_json_script_vars_also_in_properties() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Player", "Node2D");
        let child_id = tree.add_child(root, child).unwrap();

        let script_src = "\
extends Node2D
var speed = 200
func _ready():
    self.speed = 300
";
        let script = GDScriptNodeInstance::from_source(script_src, child_id).unwrap();
        tree.attach_script(child_id, Box::new(script));

        gdscene::LifecycleManager::enter_tree(&mut tree, child_id);

        let output = dump_tree_json(&tree);
        let children = output["children"].as_array().unwrap();
        let player = &children[0];

        // Both script_vars and properties should have speed=300
        assert_eq!(player["script_vars"]["speed"]["value"], json!(300));
        assert_eq!(player["properties"]["speed"]["value"], json!(300));
    }

    #[test]
    fn run_main_loop_with_trace_captures_every_frame() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let child = Node::new("Child", "Node");
        tree.add_child(root, child).unwrap();

        let mut main_loop = MainLoop::new(tree);
        let trace = run_main_loop(&mut main_loop, 2, 1.0 / 60.0, true);

        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0]["frame"], json!(1));
        assert_eq!(trace[1]["frame"], json!(2));
        assert_eq!(trace[0]["tree"]["children"][0]["name"], json!("Child"));
    }
}
