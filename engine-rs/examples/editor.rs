//! Patina Editor — interactive scene editor with browser UI.
//!
//! Loads a `.tscn` scene file and serves a web-based editor interface
//! for inspecting and modifying the scene tree in real time.
//!
//! Usage:
//!   cargo run --example editor
//!   cargo run --example editor -- fixtures/scenes/demo_2d.tscn
//!   cargo run --example editor -- --port 9090 fixtures/scenes/demo_2d.tscn

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gdcore::math::Color;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::renderer::FrameBuffer;
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::SoftwareRenderer;
use gdscene::lifecycle::LifecycleManager;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdserver2d::viewport::Viewport;

const DEFAULT_SCENE: &str = "fixtures/scenes/demo_2d.tscn";
const DEFAULT_PORT: u16 = 8080;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let mut port = DEFAULT_PORT;
    let mut scene_path = DEFAULT_SCENE.to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                if i < args.len() {
                    port = args[i].parse().expect("invalid port number");
                }
            }
            arg if !arg.starts_with('-') => {
                scene_path = arg.to_string();
            }
            other => {
                eprintln!("Unknown argument: {other}");
            }
        }
        i += 1;
    }

    // Load the scene.
    let source = std::fs::read_to_string(&scene_path)
        .unwrap_or_else(|e| panic!("Failed to read {scene_path}: {e}"));
    let packed = PackedScene::from_tscn(&source)
        .unwrap_or_else(|e| panic!("Failed to parse {scene_path}: {e}"));

    // Instance into a scene tree.
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root_id, &packed)
        .unwrap_or_else(|e| panic!("Failed to instance scene: {e}"));

    // Run lifecycle (enter_tree + ready).
    LifecycleManager::enter_tree(&mut tree, scene_root);

    // Render an initial frame.
    let mut renderer = SoftwareRenderer::new();
    let viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.05, 0.05, 0.1));
    let initial_frame = capture_frame(&mut renderer, &viewport);

    // Set up editor state with texture cache rooted at the scene file's directory.
    let mut state = EditorState::new(tree);
    state.frame_buffer = Some(initial_frame);
    let project_root = std::path::Path::new(&scene_path)
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_string_lossy()
        .to_string();
    state.texture_cache = gdeditor::texture_cache::TextureCache::new(&project_root);

    // Start the editor server.
    let handle = EditorServerHandle::start(port, state);

    println!("Patina Editor running at http://localhost:{port}/editor");
    println!("Scene: {scene_path}");
    println!("Press Ctrl+C to quit.");

    // Set up Ctrl+C handler.
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    setup_ctrlc(&r);

    // Main loop: re-render, and run game loop when playing.
    let mut frame_count: u64 = 0;
    let mut last_runtime_tick = std::time::Instant::now();
    while running.load(Ordering::SeqCst) {
        let is_runtime_running = {
            let state = handle.state().lock().unwrap();
            state.is_running && !state.is_paused
        };
        if is_runtime_running {
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(last_runtime_tick);
            if elapsed < Duration::from_millis(16) {
                std::thread::sleep(Duration::from_millis(16) - elapsed);
            }
            last_runtime_tick = std::time::Instant::now();
            {
                let mut state = handle.state().lock().unwrap();
                let delta = state.delta_time;
                if let Some(ref mut tree) = state.run_scene_tree {
                    let ids = tree.all_nodes_in_tree_order();
                    for id in &ids {
                        let velocity = {
                            let node = match tree.get_node(*id) {
                                Some(n) => n,
                                None => continue,
                            };
                            match node.get_property("velocity") {
                                gdvariant::Variant::Vector2(v) => Some(v),
                                _ => None,
                            }
                        };
                        if let Some(vel) = velocity {
                            let new_pos = {
                                let node = tree.get_node(*id).unwrap();
                                match node.get_property("position") {
                                    gdvariant::Variant::Vector2(pos) => {
                                        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(
                                            pos.x + vel.x * delta as f32,
                                            pos.y + vel.y * delta as f32,
                                        ))
                                    }
                                    _ => continue,
                                }
                            };
                            if let Some(node) = tree.get_node_mut(*id) {
                                node.set_property("position", new_pos);
                            }
                        }
                    }
                    tree.process_animations(delta);
                    tree.process_tweens(delta);
                    tree.process_all_scripts_process(delta);
                    tree.process_frame();
                }
                state.runtime_frame_count += 1;
            }
            let fb = {
                let state = handle.state().lock().unwrap();
                let zoom = state.viewport_zoom;
                let pan = state.viewport_pan;
                if let Some(ref tree) = state.run_scene_tree {
                    gdeditor::scene_renderer::render_scene_with_zoom_pan(
                        tree, None, WIDTH, HEIGHT, zoom, pan,
                    )
                } else {
                    gdeditor::scene_renderer::render_scene_with_zoom_pan(
                        &state.scene_tree,
                        state.selected_node,
                        WIDTH,
                        HEIGHT,
                        zoom,
                        pan,
                    )
                }
            };
            handle.update_frame(fb);
        } else {
            std::thread::sleep(Duration::from_millis(50));
            if frame_count % 2 == 0 {
                let fb = {
                    let state = handle.state().lock().unwrap();
                    let selected = state.selected_node;
                    let zoom = state.viewport_zoom;
                    let pan = state.viewport_pan;
                    gdeditor::scene_renderer::render_scene_with_zoom_pan(
                        &state.scene_tree,
                        selected,
                        WIDTH,
                        HEIGHT,
                        zoom,
                        pan,
                    )
                };
                handle.update_frame(fb);
            }
        }
        frame_count += 1;
        if frame_count % 200 == 0 {
            let state = handle.state().lock().unwrap();
            let ri = if state.is_running {
                format!(
                    " | PLAYING frame {} {}",
                    state.runtime_frame_count,
                    if state.is_paused { "(PAUSED)" } else { "" }
                )
            } else {
                String::new()
            };
            println!(
                "[Frame {frame_count}] Nodes: {} | Undo stack: {} | Selected: {:?}{ri}",
                state.scene_tree.node_count(),
                state.undo_stack.len(),
                state.selected_node
            );
        }
    }

    println!("\nShutting down editor...");
    handle.stop();
}

/// Sets a Ctrl+C handler that flips the `running` flag to false.
fn setup_ctrlc(running: &Arc<AtomicBool>) {
    RUNNING_PTR.store(Arc::as_ptr(running) as usize, Ordering::SeqCst);
    std::mem::forget(Arc::clone(running));

    #[cfg(unix)]
    // SAFETY: We register a POSIX signal handler that only writes to an atomic bool.
    // The pointer stored in RUNNING_PTR is valid for the program lifetime
    // because we leaked an Arc reference above.
    unsafe {
        extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        signal(2 /* SIGINT */, sigint_handler);
    }
}

static RUNNING_PTR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(unix)]
extern "C" fn sigint_handler(_sig: i32) {
    let ptr = RUNNING_PTR.load(Ordering::SeqCst);
    if ptr != 0 {
        // SAFETY: ptr was stored from a valid Arc<AtomicBool> that we leaked.
        let flag = unsafe { &*(ptr as *const AtomicBool) };
        flag.store(false, Ordering::SeqCst);
    }
}
