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

    // Set up editor state.
    let mut state = EditorState::new(tree);
    state.frame_buffer = Some(initial_frame);

    // Start the editor server.
    let handle = EditorServerHandle::start(port, state);

    println!("Patina Editor running at http://localhost:{port}/editor");
    println!("Scene: {scene_path}");
    println!("Press Ctrl+C to quit.");

    // Set up Ctrl+C handler.
    let running = Arc::new(AtomicBool::new(true));
    let r = Arc::clone(&running);
    setup_ctrlc(&r);

    // Main loop: re-render only when scene changes (dirty flag).
    let mut frame_count: u64 = 0;
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));

        // Re-render every ~100ms (every other 50ms tick). Lock briefly, release before encoding.
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
                // Lock released at end of block
            };
            handle.update_frame(fb);
        }
        frame_count += 1;

        if frame_count % 200 == 0 {
            let state = handle.state().lock().unwrap();
            println!(
                "[Frame {frame_count}] Nodes: {} | Undo stack: {} | Selected: {:?}",
                state.scene_tree.node_count(),
                state.undo_stack.len(),
                state.selected_node,
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
