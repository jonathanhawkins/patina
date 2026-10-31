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
//! patina-runner <project.godot> --trace-out <file> [--frames N]
//! patina-runner <scene.tscn> --frames N --frame-out <file.png|file.ppm>
//! ```
//!
//! `--trace-out` writes the conformance trace envelope the conformance gate
//! diffs against headless Godot, instead of the tree dump (pat-mgixk.403).
//!
//! `--frame-out` writes the last frame's 2D canvas as a PNG or a binary PPM
//! (pat-mgixk.28). Before this the runner emitted tree JSON and nothing else:
//! the only node-to-draw-command producer in the engine lived in `gdeditor`,
//! so a 2D game run outside the editor's Play tab drew no pixels at all. The
//! builder now lives in `gdscene::canvas_render` and this is one of its two
//! runtime drivers.

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
    /// Whether `--frames` was given explicitly. `--trace-out` falls back to
    /// [`conformance::DEFAULT_FRAMES`] rather than the tree-dump default of 10,
    /// so an envelope written without `--frames` is comparable with the
    /// checked-in goldens instead of being silently two frames short.
    frames_explicit: bool,
    /// Delta time per frame in seconds (default 1/60).
    delta: f64,
    /// Whether to emit a per-frame trace of tree state.
    trace_frames: bool,
    /// Whether to emit the lifecycle event trace (notifications, script calls, signals).
    event_trace: bool,
    /// Where to write the conformance trace envelope, when asked for
    /// (pat-mgixk.403). Only meaningful for a `project.godot` target.
    trace_out: Option<PathBuf>,
    /// The `--debug-*` overlays the editor's Debug menu launched us with.
    debug: patina_runner::debug_flags::DebugFlags,
    /// Where to write the last frame's 2D canvas as a binary PPM, when asked
    /// for (pat-mgixk.28).
    frame_out: Option<PathBuf>,
    /// Framebuffer size for `--frame-out`, defaulting to Godot's own default
    /// viewport (1152x648).
    frame_size: (u32, u32),
}

/// Parses command-line arguments manually (no extra dependency).
fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        return Err(format!(
            "Usage: {} <scene.tscn|project.godot|game.pck> [--main-pack PACK] \
             [--frames N] [--delta D] [--trace-out FILE] \
             [--frame-out FILE.png|FILE.ppm] \
             [--frame-size WxH]",
            args[0]
        ));
    }

    // Godot spells the packed-game entry point `--main-pack <path>`
    // (pat-s3rrv.34). Supporting it means the first argument is optional, so
    // when it starts with `--` the positional target is left empty and flag
    // parsing begins at index 1 instead of 2.
    let starts_with_flag = args[1].starts_with("--");
    let mut scene_path = if starts_with_flag {
        String::new()
    } else {
        args[1].clone()
    };
    let mut frames: u64 = 10;
    let mut frames_explicit = false;
    let mut delta: f64 = 1.0 / 60.0;
    let mut trace_frames = false;
    let mut event_trace = false;
    let mut trace_out: Option<PathBuf> = None;
    let mut frame_out: Option<PathBuf> = None;
    // Godot's default viewport, so a frame dumped without `--frame-size`
    // matches what the project would open at.
    let mut frame_size: (u32, u32) = (1152, 648);
    let mut debug = patina_runner::debug_flags::DebugFlags::default();

    let mut i = if starts_with_flag { 1 } else { 2 };
    while i < args.len() {
        match args[i].as_str() {
            "--frames" => {
                i += 1;
                frames = args
                    .get(i)
                    .ok_or("--frames requires a value")?
                    .parse()
                    .map_err(|e| format!("invalid --frames value: {e}"))?;
                frames_explicit = true;
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
            "--trace-out" => {
                i += 1;
                trace_out = Some(PathBuf::from(
                    args.get(i).ok_or("--trace-out requires a path")?,
                ));
            }
            "--frame-out" => {
                i += 1;
                frame_out = Some(PathBuf::from(
                    args.get(i).ok_or("--frame-out requires a path")?,
                ));
            }
            "--frame-size" => {
                i += 1;
                let spec = args.get(i).ok_or("--frame-size requires WxH")?;
                let (w, h) = spec
                    .split_once(['x', 'X'])
                    .ok_or("--frame-size must look like 320x200")?;
                frame_size = (
                    w.trim()
                        .parse()
                        .map_err(|e| format!("invalid --frame-size width: {e}"))?,
                    h.trim()
                        .parse()
                        .map_err(|e| format!("invalid --frame-size height: {e}"))?,
                );
                if frame_size.0 == 0 || frame_size.1 == 0 {
                    return Err("--frame-size must be non-zero in both axes".to_string());
                }
            }
            "--main-pack" => {
                i += 1;
                scene_path = args.get(i).ok_or("--main-pack requires a path")?.clone();
            }
            other => {
                // ⚠️ The editor's Debug menu launches Play with these
                // (`DebugToggles::play_session_flags`), and this arm used to
                // reject them — so checking ANY item in that menu made Play
                // fail with `unknown argument: --debug-navigation` instead of
                // starting with the overlay on (pat-s3rrv.82).
                //
                // Still an error for anything genuinely unknown: `apply`
                // reports whether it recognised the flag rather than swallowing
                // it, so a typo is not silently ignored.
                if !debug.apply(other) {
                    return Err(format!("unknown argument: {other}"));
                }
            }
        }
        i += 1;
    }

    if scene_path.is_empty() {
        return Err("no scene, project or pack given (pass a path or --main-pack)".to_string());
    }

    Ok(Args {
        scene_path,
        frames,
        frames_explicit,
        delta,
        trace_frames,
        event_trace,
        trace_out,
        debug,
        frame_out,
        frame_size,
    })
}

/// Writes the tree's 2D canvas to `path`, as PNG or binary PPM by extension
/// (pat-mgixk.28; PNG added by pat-mgixk.46).
///
/// 🔑 This is the whole runtime 2D path in four calls, and all four live below
/// `gdeditor`: walk the tree into a draw list, move it into the current
/// `Camera2D`'s view, rasterize, encode. The editor composes the SAME four with
/// its own overlays on top — there is one builder now, not an editor one and a
/// missing runtime one.
///
/// ⚠️ The camera transform is applied to the list, not folded into the
/// rasterizer: `apply_camera2d_view` is what honours a `CanvasLayer`'s
/// `follow_viewport` and a `Fixed` layer's exemption, so a HUD stays put while
/// the world scrolls. Skipping it would put every item at its world position
/// and look correct only while the camera sits at the origin.
fn write_frame_out(
    main_loop: &MainLoop,
    project_dir: &Path,
    path: &Path,
    size: (u32, u32),
) -> Result<(), String> {
    use gdcore::math::{Color, Vector2};

    let (width, height) = size;
    let tree = main_loop.tree();
    let mut items = gdscene::canvas_render::collect_canvas_items(tree);
    if let Some(view) = gdscene::canvas_render::camera2d_view_transform(
        tree,
        Vector2::new(width as f32, height as f32),
        None,
        0.0,
    ) {
        gdscene::canvas_render::apply_camera2d_view(&mut items, view);
    }
    // `res://` textures resolve against the project directory, exactly as the
    // audio base dir above does.
    let mut textures =
        gdscene::texture_cache::TextureCache::new(project_dir.to_string_lossy().to_string());
    let frame = gdscene::canvas_render::rasterize_render_list(
        &items,
        width,
        height,
        // Godot's default clear is opaque black; a transparent clear would make
        // "the sprite drew" indistinguishable from "nothing drew" in a PPM,
        // which has no alpha channel.
        Color::new(0.0, 0.0, 0.0, 1.0),
        Some(&mut textures),
    );
    // PNG or PPM by extension (pat-mgixk.46). PPM was the original format and
    // stays the default for an unrecognised extension, so every existing
    // `--frame-out out.ppm` caller is untouched; PNG is what a test can decode
    // back into pixels with `gdrender2d::texture::load_png`, and what anything
    // outside this repo can open.
    let is_png = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    let target = path.to_string_lossy();
    if is_png {
        frame
            .save_png(&target)
            .map_err(|e| format!("could not write {}: {e}", path.display()))
    } else {
        gdrender2d::export::save_ppm(&frame, &target)
            .map_err(|e| format!("could not write {}: {e}", path.display()))
    }
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

/// Attaches each node's `_script_path` GDScript.
///
/// Delegates to [`project_boot::attach_scripts`], the single implementation
/// (pat-mgixk.414). This path used to carry its own copy while the
/// project-directory boot carried none at all, which is how a booted project
/// ran every main-scene node with no script attached.
fn attach_scripts(tree: &mut SceneTree, project_dir: &Path) {
    project_boot::attach_scripts(tree, project_dir);
}

/// Resolves a `res://` path to an absolute filesystem path.
///
/// Delegates to [`project_boot::resolve_res_path`]; see the note above.
fn resolve_res_path(project_dir: &Path, res_path: &str) -> PathBuf {
    project_boot::resolve_res_path(project_dir, res_path)
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

    // pat-mgixk.403: `--trace-out` records the conformance trace envelope from
    // this very boot and writes that instead of the tree dump. It calls the
    // same `conformance::capture_project` the conformance suite runs, so the
    // gate and the shipping runner can never drift apart.
    if let Some(out) = &args.trace_out {
        let frames = if args.frames_explicit {
            args.frames
        } else {
            patina_runner::conformance::DEFAULT_FRAMES
        };
        match patina_runner::conformance::capture_project(&project_dir, frames, args.delta) {
            Ok(envelope) => {
                let text = serde_json::to_string_pretty(&envelope.to_json())
                    .expect("JSON serialization failed");
                if let Err(e) = fs::write(out, format!("{text}\n")) {
                    eprintln!("Error writing '{}': {e}", out.display());
                    process::exit(1);
                }
                eprintln!("wrote conformance envelope to {}", out.display());
            }
            Err(e) => {
                eprintln!("Error capturing '{}': {e}", project_dir.display());
                process::exit(1);
            }
        }
        return;
    }

    let booted = match project_boot::boot_project(&project_dir) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error booting project '{}': {e}", project_dir.display());
            process::exit(1);
        }
    };
    run_booted_project(booted, &project_dir, args);
}

/// Boots an exported game from a `.pck` (pat-s3rrv.34).
///
/// Godot ships a game as a pack and boots it with no loose project tree at all;
/// `boot_project_from_pck` already mounted-and-parsed that case, but nothing on
/// the runner's CLI ever reached it, so `patina-runner game.pck` was not a
/// runnable command. `res://` still points at the pack's own directory, so a
/// file deliberately shipped beside the pack keeps resolving.
fn run_project_from_pck(pck_path: &Path, args: &Args) {
    let booted = match project_boot::boot_project_from_pck(pck_path, None) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error booting pack '{}': {e}", pck_path.display());
            process::exit(1);
        }
    };
    // Anything not in the pack resolves beside it, matching the fallback root
    // `boot_project_from_pck` installs.
    let base_dir = pck_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    run_booted_project(booted, &base_dir, args);
}

/// Runs a booted project's lifecycle, frames and JSON dump.
///
/// Shared by the `project.godot` and `.pck` entry points so a packed game runs
/// through exactly the same loop configuration — physics settings, input map
/// and physics-body registration included — rather than a parallel copy that
/// could drift.
fn run_booted_project(booted: project_boot::BootedProject, project_dir: &Path, args: &Args) {
    let project_dir = project_dir.to_path_buf();
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
        "booted project"
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
    if let Some(ref path) = args.frame_out {
        if let Err(e) = write_frame_out(&main_loop, &project_dir, path, args.frame_size) {
            eprintln!("Error writing frame: {e}");
            process::exit(1);
        }
    }

    let mut output = json!({
        "project_dir": project_dir.display().to_string(),
        "frame_count": main_loop.frame_count(),
        "physics_time": main_loop.physics_time(),
        "process_time": main_loop.process_time(),
        "tree": dump_tree_json(main_loop.tree()),
    });
    // Report the debug overlays this session was launched with (pat-s3rrv.82),
    // so a harness can confirm the editor's Debug menu reached the runner
    // rather than being dropped on the way.
    //
    // ⚠️ Only the ENABLED ones are emitted. An always-present block of five
    // `false`s would change the output of every existing run and could break a
    // consumer asserting on the exact report.
    for (name, enabled) in args.debug.as_pairs() {
        if enabled {
            output[name] = Value::Bool(true);
        }
    }
    // `--debug-navigation` also emits the navmesh itself, so a harness can
    // assert the geometry reached the running scene and not merely the flag
    // (pat-s3rrv.82). Same discipline as the flags above: emitted ONLY when the
    // flag is on, so no existing run's output changes.
    if args.debug.navigation {
        output["navigation_draw"] =
            patina_runner::navigation_debug::navmesh_draw_items(main_loop.tree());
    }
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

    // The conformance envelope is defined over a whole project (autoloads,
    // input map, declared main scene), so refuse rather than silently write an
    // envelope that no golden can be compared against.
    if args.trace_out.is_some() {
        eprintln!("Error: --trace-out needs a project.godot target, not a loose scene or pack");
        process::exit(1);
    }

    // pat-s3rrv.34: a `.pck` argument boots the exported game inside it.
    if arg_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pck"))
    {
        run_project_from_pck(arg_path, &args);
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
    if let Some(ref path) = args.frame_out {
        if let Err(e) = write_frame_out(&main_loop, &project_dir, path, args.frame_size) {
            eprintln!("Error writing frame: {e}");
            process::exit(1);
        }
    }

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
    // Report the debug overlays this session was launched with (pat-s3rrv.82),
    // so a harness can confirm the editor's Debug menu reached the runner
    // rather than being dropped on the way.
    //
    // ⚠️ Only the ENABLED ones are emitted. An always-present block of five
    // `false`s would change the output of every existing run and could break a
    // consumer asserting on the exact report.
    for (name, enabled) in args.debug.as_pairs() {
        if enabled {
            output[name] = Value::Bool(true);
        }
    }
    // `--debug-navigation` also emits the navmesh itself, so a harness can
    // assert the geometry reached the running scene and not merely the flag
    // (pat-s3rrv.82). Same discipline as the flags above: emitted ONLY when the
    // flag is on, so no existing run's output changes.
    if args.debug.navigation {
        output["navigation_draw"] =
            patina_runner::navigation_debug::navmesh_draw_items(main_loop.tree());
    }
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
