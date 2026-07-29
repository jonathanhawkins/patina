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

mod class_defaults;

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use gdscene::node::NodeId;
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::trace::TraceEventType;
use gdscene::{add_packed_scene_to_tree, LifecycleManager, MainLoop, PackedScene};
use gdvariant::serialize::to_json;
use gdvariant::Variant;
use patina_runner::project_boot;
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
    /// Whether to emit the lifecycle event trace (notifications, script calls, signals).
    event_trace: bool,
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
    let mut event_trace = false;

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
            "--event-trace" => {
                event_trace = true;
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
        event_trace,
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
    // Only include properties that are known Godot class properties with
    // non-default values, matching the oracle property dump format.
    let mut props = serde_json::Map::new();
    let sorted_props: BTreeMap<&String, &gdvariant::Variant> = node.properties().collect();
    for (key, value) in sorted_props {
        if class_defaults::should_output_property(node.class_name(), key, value) {
            props.insert(key.clone(), to_json(value));
        }
    }

    // Collect script variables if a script is attached.
    // Script variables are also merged into properties, matching Godot's
    // behavior where script-exported vars appear in the property list.
    let mut script_vars = serde_json::Map::new();
    if let Some(script) = tree.get_script(id) {
        for prop_info in script.list_properties() {
            let json_val = to_json(&prop_info.default_value);
            script_vars.insert(prop_info.name.clone(), json_val.clone());
            // Merge script vars into properties so they match
            // the oracle property dump format (Godot 4.6.1+).
            props.insert(prop_info.name.clone(), json_val);
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
        // Skip nodes that already have a script attached (e.g. autoload
        // singletons instantiated during the autoload boot step) so we don't
        // re-parse and clobber their already-run _ready state.
        if tree.get_script(node_id).is_some() {
            continue;
        }
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
    for ancestor in project_dir.ancestors() {
        let candidate = ancestor.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }

    if let Ok(cwd) = env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    project_dir.join(relative)
}

/// Serializes the EventTrace events into a JSON array.
fn serialize_event_trace(tree: &SceneTree) -> Value {
    let events: Vec<Value> = tree
        .event_trace()
        .events()
        .iter()
        .map(|ev| {
            json!({
                "event_type": match ev.event_type {
                    TraceEventType::Notification => "notification",
                    TraceEventType::SignalEmit => "signal_emit",
                    TraceEventType::ScriptCall => "script_call",
                    TraceEventType::ScriptReturn => "script_return",
                },
                "node_path": ev.node_path,
                "detail": ev.detail,
                "frame": ev.frame,
            })
        })
        .collect();
    Value::Array(events)
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
        // Godot: `SceneTree.quit()` ends the loop at the frame boundary — the
        // frame that called it finishes, then no further frames run. Before
        // this, `should_quit` was set and nothing ever read it, so a scene that
        // quit kept simulating to the requested frame count (pat-0q2kx.47).
        if main_loop.tree().should_quit() {
            break;
        }
    }
    frame_trace
}

/// Ends the process with the exit code a script requested through
/// `get_tree().quit(code)` (pat-0q2kx.47).
///
/// Called after the run's JSON has been written, so a scene that quits still
/// produces its output — Godot likewise finishes the frame and flushes before
/// the process ends. A run that never quit returns normally and exits 0.
fn exit_if_quit_requested(main_loop: &MainLoop) {
    if main_loop.tree().should_quit() {
        process::exit(main_loop.tree().exit_code());
    }
}

/// Boots the runner from a `project.godot` (pat-nzgdu.17): parses the project,
/// installs `res://` roots, instantiates autoloads + the declared main scene,
/// runs enter_tree/_ready lifecycle and frames, then dumps the tree — mirroring
/// the raw-`.tscn` path but driven by the project configuration.
fn run_project_from_godot(arg_path: &Path, args: &Args) {
    let project_dir = if arg_path.is_dir() {
        arg_path.to_path_buf()
    } else {
        arg_path.parent().unwrap_or(Path::new(".")).to_path_buf()
    };

    let booted = match project_boot::boot_project(&project_dir) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error booting project '{}': {e}", project_dir.display());
            process::exit(1);
        }
    };

    let mut tree = booted.tree;
    if args.event_trace {
        tree.event_trace_mut().enable();
    }

    // Run lifecycle: autoloads first (Godot boots them before the main scene),
    // then the declared main scene.
    for (_, node_id) in &booted.autoload_roots {
        LifecycleManager::enter_tree(&mut tree, *node_id);
    }
    if let Some(main_root) = booted.main_scene_root {
        LifecycleManager::enter_tree(&mut tree, main_root);
    }

    tracing::info!(
        node_count = tree.node_count(),
        autoloads = booted.autoload_roots.len(),
        actions = booted.input_map.actions().count(),
        "booted project from project.godot"
    );

    let mut main_loop = MainLoop::new(tree);
    // `res://` audio paths resolve against the project directory, not the CWD
    // (pat-s3rrv.5) — without this the runner loads no audio unless it happens
    // to be launched from inside the project.
    main_loop
        .audio_bridge_mut()
        .set_base_dir(project_dir.clone());
    // pat-nzgdu.15: honor project.godot physics config (tick rate + 2D/3D
    // gravity) before running any frames, mirroring Godot's boot.
    main_loop.configure_from_project_settings(&booted.settings);
    // Install the project's `[input]` action map (pat-0q2kx.22). `boot_project`
    // has always parsed it — the line above even logs its action count — but
    // nothing ever handed it to the loop, so every `Input.is_action_pressed`
    // in a running project resolved against an empty map.
    main_loop.set_input_map(booted.input_map);
    // Register 2D physics bodies so RigidBody2D/etc. actually simulate under the
    // honored gravity (2D bodies are not auto-registered by `step`).
    main_loop.register_physics_bodies();
    let frame_trace = run_main_loop(&mut main_loop, args.frames, args.delta, args.trace_frames);

    let mut output = json!({
        "project_dir": project_dir.display().to_string(),
        "frame_count": main_loop.frame_count(),
        "physics_time": main_loop.physics_time(),
        "process_time": main_loop.process_time(),
        "tree": dump_tree_json(main_loop.tree()),
    });
    if args.trace_frames {
        output["frame_trace"] = Value::Array(frame_trace);
    }
    if args.event_trace {
        output["event_trace"] = serialize_event_trace(main_loop.tree());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("JSON serialization failed")
    );
    exit_if_quit_requested(&main_loop);
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

    // pat-nzgdu.17: if the argument is a project directory or a `project.godot`
    // file, boot from project.godot (main scene, autoloads, input map,
    // ProjectSettings, res:// root) rather than treating it as a raw `.tscn`.
    let arg_path = Path::new(&args.scene_path);
    if arg_path.is_dir() || arg_path.file_name().is_some_and(|n| n == "project.godot") {
        run_project_from_godot(arg_path, &args);
        return;
    }

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

    // Create the scene tree.
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();

    // Resolve the project directory from the scene path. Used for res://
    // resolution of both autoload resources and node scripts.
    let resolved_scene_path = Path::new(&args.scene_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(&args.scene_path));
    let project_dir = resolved_scene_path
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    // Enable event tracing before any lifecycle runs so both autoload and
    // main-scene _ready callbacks are captured.
    if args.event_trace {
        tree.event_trace_mut().enable();
    }

    // Instantiate project autoloads under /root BEFORE the main scene, so the
    // main scene's _ready can reference /root/<Autoload> — matching Godot's
    // boot order (main/main.cpp:4367-4433).
    let autoloads = gdscene::autoload::boot_autoloads_from_dir(&mut tree, &project_dir);
    if !autoloads.is_empty() {
        tracing::info!(count = autoloads.len(), "instantiated project autoloads");
    }

    // Instance the main scene under /root.
    let scene_root_id = match add_packed_scene_to_tree(&mut tree, root_id, &packed_scene) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Error instancing scene: {e}",);
            process::exit(1);
        }
    };

    // Attach GDScript files to main-scene nodes that have _script_path.
    // Autoload nodes already carry their scripts and are skipped by attach_scripts.
    attach_scripts(&mut tree, &project_dir);

    // Run main-scene lifecycle: enter_tree + ready.
    LifecycleManager::enter_tree(&mut tree, scene_root_id);

    tracing::info!(node_count = tree.node_count(), "scene instanced");

    // Create the main loop and run frames.
    let mut main_loop = MainLoop::new(tree);
    // `res://` audio paths resolve against the project directory, not the CWD
    // (pat-s3rrv.5).
    main_loop
        .audio_bridge_mut()
        .set_base_dir(project_dir.clone());
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
    let mut output = output;
    if args.trace_frames {
        output["frame_trace"] = Value::Array(frame_trace);
    }
    if args.event_trace {
        output["event_trace"] = serialize_event_trace(main_loop.tree());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("JSON serialization failed")
    );
    exit_if_quit_requested(&main_loop);
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

    /// Script vars appear in both script_vars and properties (matching Godot 4.6.1 oracle format).
    #[test]
    fn dump_tree_json_script_vars_merged_into_properties() {
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

        // script_vars should have speed=300
        assert_eq!(player["script_vars"]["speed"]["value"], json!(300));
        // properties should ALSO have speed (Godot 4.6.1+ reports script vars as properties)
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

    #[test]
    fn resolve_res_path_finds_fixture_script_via_ancestor_search() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let resolved = resolve_res_path(
            &manifest_dir.join("../fixtures/scenes"),
            "res://fixtures/scripts/test_movement.gd",
        );

        assert!(
            resolved.ends_with("fixtures/scripts/test_movement.gd"),
            "expected fixture script path, got {}",
            resolved.display()
        );
        assert!(resolved.exists(), "resolved path must exist");
    }
}
