//! Patina Editor — interactive scene editor with browser UI.
//!
//! Loads a `.tscn` scene file and serves a web-based editor interface
//! for inspecting and modifying the scene tree in real time.
//!
//! ## Current limitations
//!
//! - **Play/stop mode (pat-zdc)**: The editor has `is_running`/`is_paused`
//!   state wired into the main loop, but there is no formal play/stop/edit-lock
//!   API exposed via HTTP endpoints. Formalizing a play-mode workflow with
//!   proper edit-lock semantics is deferred to post-runtime-parity work.
//!
//! - **Multi-tab / session switching (pat-vww)**: The editor is single-scene.
//!   Opening, switching, and closing multiple scene tabs is not supported.
//!   Multi-tab session management is deferred to post-runtime-parity work.
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
use gdeditor::idle_render::ViewportRenderSignature;
use gdrender2d::test_adapter::capture_frame;
use gdrender2d::SoftwareRenderer;
use gdscene::lifecycle::LifecycleManager;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;
use gdvariant::Variant;
use gdserver2d::viewport::Viewport;

const DEFAULT_SCENE: &str = "fixtures/scenes/demo_2d.tscn";
const DEFAULT_PORT: u16 = 8080;
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
/// How often the idle editor re-renders even when nothing changed — a low-
/// frequency safety net (pat-5xrsr) for any frame-affecting state the render
/// signature doesn't model. Low enough that an idle editor stays well under 5%
/// CPU, high enough that nothing stays stale for long.
const IDLE_KEEPALIVE: Duration = Duration::from_secs(2);

/// A render job produced under the editor state lock and finished after releasing
/// it. For 3D, only the tree-dependent inputs are snapshotted under the lock (a
/// viewport clone + render list); the GPU submit, blocking readback, overlay
/// rasterization, and BMP encode run **off the lock** so `/api` handlers stay
/// responsive during interaction (pat-ism62 lock-hold reduction). The 2D path is
/// cheap and stays rendered under the lock.
enum RenderJob {
    /// GPU 3D viewport: snapshot to render off the lock (viewport + render list).
    Gpu {
        vp: gdeditor::viewport_3d::Viewport3D,
        inputs: gdeditor::viewport_3d_render::Editor3DRenderInputs,
    },
    /// Software-rendered 2D viewport (already rasterized under the lock).
    Cpu(gdrender2d::FrameBuffer),
}

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

    // Attach demo scripts so Script mode shows real content for a selected node
    // (pat-qtmbv): Player -> player.gd, Enemy -> enemy_spawner.gd. The selection
    // handler reads `_script_path` and loads it via GET /api/script (res://
    // resolves against the cwd, where fixtures/scripts/*.gd live).
    for nid in tree.all_nodes_in_tree_order() {
        let script = match tree.get_node(nid).map(|n| n.name().to_string()).as_deref() {
            Some("Player") => Some("res://fixtures/scripts/player.gd"),
            Some("Enemy") => Some("res://fixtures/scripts/enemy_spawner.gd"),
            _ => None,
        };
        if let Some(path) = script {
            if let Some(node) = tree.get_node_mut(nid) {
                node.set_property("_script_path", Variant::String(path.to_string()));
            }
        }
    }

    // Run lifecycle (enter_tree + ready).
    LifecycleManager::enter_tree(&mut tree, scene_root);

    // Render an initial frame.
    let mut renderer = SoftwareRenderer::new();
    let viewport = Viewport::new(WIDTH, HEIGHT, Color::rgb(0.05, 0.05, 0.1));
    let initial_frame = capture_frame(&mut renderer, &viewport);

    // Set up editor state with texture cache rooted at the scene file's directory.
    let mut state = EditorState::new(tree);
    state.scene_file = Some(scene_path.clone());
    state.frame_buffer = Some(initial_frame);
    // Root res:// at the current working directory so the runtime resolves
    // res:// scripts/textures the same way the editor's /api/script and
    // /api/filesystem do (both cwd-based). This keeps a node's _script_path
    // (e.g. res://fixtures/scripts/player.gd) loadable in BOTH the editor's
    // Script view and the running game (pat-xwdlg) instead of the runtime
    // doubling the scene-dir prefix.
    let project_root = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    state.texture_cache = gdeditor::texture_cache::TextureCache::new(&project_root);
    // Give the Create Node dialog a project root so favorites/recent persist
    // under <root>/.patina/editor, like Godot's per-project settings dir.
    state.project_root = Some(project_root.clone());

    // Start the editor server.
    let handle = EditorServerHandle::start(port, state);

    // Interactive 3D-viewport render tuning (see /api/viewport/3d/perf to re-tune
    // live). The 3D viewport now rasterizes meshes on the GPU (a persistent wgpu
    // renderer, pat-ism62), whose cost barely scales with pixel count — so the
    // moving camera renders at FULL resolution (scale 1.0), not the reduced scale
    // the old software rasterizer needed. The loop polls every 2ms during motion.
    // These are process-global knobs set only here, so test servers keep the safe
    // defaults. (If no GPU adapter is present the render path falls back to the
    // software rasterizer; dial PERF_SCALE down via the API for that case.)
    {
        use std::sync::atomic::Ordering::Relaxed;
        gdeditor::editor_server::PERF_SCALE_X1000.store(1000, Relaxed);
        gdeditor::editor_server::PERF_TICK_MS.store(2, Relaxed);
    }

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
    // Render-on-change gating for the idle editor (pat-5xrsr): the signature of
    // the last frame we actually rendered, and when we rendered it. While idle we
    // re-render only when the signature changes or the keepalive elapses.
    let mut last_render_sig: Option<ViewportRenderSignature> = None;
    let mut last_render_at = std::time::Instant::now();
    // Adaptive idle cadence: after we actually render a changed frame (e.g. the
    // user is dragging/orbiting the camera), keep polling fast for a short window
    // so a continuous interaction renders smoothly (~60 fps) instead of the 20 fps
    // a flat 50ms tick allows. When nothing changes the window lapses and we fall
    // back to the 50ms low-CPU tick — preserving the pat-5xrsr idle-CPU fix, since
    // only the *cheap* signature check polls faster, never the expensive raster.
    let mut active_render_until = std::time::Instant::now();
    // Dynamic-resolution state (interactive-render optimization): while the 3D
    // camera is actively moving we raster a smaller frame and let the browser
    // upscale it (the software rasterizer's cost scales with pixel count, so this
    // multiplies the interactive framerate). `last_frame_lowres` lets us re-render
    // once at full resolution the moment the camera stops, so the view snaps back
    // to crisp. The interactive scale is a live perf knob (PERF_SCALE_X1000,
    // default 0.4×); the browser upscales the frame with bilinear smoothing during
    // motion so the reduced resolution reads as motion blur.
    let mut last_frame_lowres = false;
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
                let input_snapshot = state.make_input_snapshot();
                if let Some(ref mut main_loop) = state.run_main_loop {
                    main_loop.set_input(input_snapshot);
                    let output = main_loop.step(delta);
                    state.runtime_frame_count = output.frame_count;
                }
                state.clear_frame_input();
            }
            let job = {
                let state = handle.state().lock().unwrap();
                let zoom = state.viewport_zoom;
                let pan = state.viewport_pan;
                // Render at the panel's real size (reported by the browser via
                // /api/viewport/resize) so the frame fills the viewport instead
                // of letterboxing a fixed 640x480 box into a centered square.
                let (rw, rh) = (state.render_width.clamp(64, 2048), state.render_height.clamp(64, 2048));
                // Render the running scene the way its game would: a 3D scene
                // (Node3D) through the GPU spatial path (pat-ism62 #3 — the same
                // persistent GPU renderer the editor viewport uses), a 2D scene
                // through the canvas rasterizer. Previously the running 3D view
                // rasterized in software at full res; routing it through the GPU
                // makes Play-on-a-3D-scene match the editor viewport's speed.
                let running_tree: &SceneTree = state
                    .run_main_loop
                    .as_ref()
                    .map(|ml| ml.tree())
                    .unwrap_or(&state.scene_tree);
                if gdeditor::scene_renderer::scene_is_3d(running_tree) {
                    // Snapshot the tree-dependent inputs under the lock; GPU-render
                    // off it (same lock-hold reduction as the editor path).
                    let mut vp = state.editor_viewport_3d.clone();
                    vp.width = rw.max(1);
                    vp.height = rh.max(1);
                    let inputs =
                        gdeditor::viewport_3d_render::editor_3d_render_inputs(&vp, running_tree);
                    RenderJob::Gpu { vp, inputs }
                } else {
                    RenderJob::Cpu(gdeditor::scene_renderer::render_runtime_frame(
                        running_tree,
                        &state.editor_viewport_3d,
                        rw,
                        rh,
                        zoom,
                        pan,
                    ))
                }
            };
            match job {
                // Running 3D scene: GPU-render off the lock, then publish BMP (the
                // Game view streams BMP, like the editor viewport).
                RenderJob::Gpu { vp, inputs } => {
                    let img = gdeditor::viewport_3d_render::render_editor_3d_frame_gpu(&vp, &inputs);
                    let bmp = gdrender2d::encode_bmp_bgra(&img);
                    handle.update_frame_bmp(bmp, img.width, img.height);
                }
                // 2D running scene: canvas rasterizer, full-quality publish.
                RenderJob::Cpu(fb) => handle.update_frame(fb),
            }
        } else {
            // Poll fast while an interaction is actively producing new frames,
            // otherwise idle at 50ms. The interaction cadence is a live perf knob
            // (PERF_TICK_MS) so the framerate benchmark can sweep it.
            let interaction_active = std::time::Instant::now() < active_render_until;
            let tick_ms = if interaction_active {
                gdeditor::editor_server::PERF_TICK_MS.load(std::sync::atomic::Ordering::Relaxed)
                    as u64
            } else {
                gdeditor::idle_render::idle_tick_sleep(false).as_millis() as u64
            };
            std::thread::sleep(Duration::from_millis(tick_ms));
            // Render-on-change (pat-5xrsr): only re-raster the idle viewport when
            // something that affects the frame actually changed (tracked by a
            // cheap signature), when an animation preview is playing, or on a
            // low-frequency keepalive. Previously this rendered the full software
            // raster ~10x/second unconditionally, so an idle editor burned
            // 30-77% CPU and orphans starved the shared build lock.
            let maybe_fb = {
                let mut state = handle.state().lock().unwrap();
                let sig = ViewportRenderSignature::from_state(&state);
                // Render every idle tick while an animation preview plays or a
                // node/gizmo drag is in progress — both change the frame live
                // without bumping the signature (drags don't push undo until
                // drag-end), so gating on the signature alone would freeze them.
                let force_active = gdeditor::idle_render::force_render_active(&state);
                // Reduce resolution only while the 3D *camera* is moving — never
                // during a node/gizmo drag or an animation preview (those set
                // `force_active` and need full precision/quality).
                let camera_lowres =
                    interaction_active && !force_active && state.editor_mode == "3d";
                // When the camera stops, the last low-res frame must be re-rendered
                // once at full resolution to sharpen — even though the render
                // signature is unchanged (so `should_render_idle` alone wouldn't
                // fire).
                let sharpen = last_frame_lowres && !camera_lowres;
                if sharpen
                    || gdeditor::idle_render::should_render_idle(
                        last_render_sig.as_ref(),
                        &sig,
                        force_active,
                        last_render_at.elapsed(),
                        IDLE_KEEPALIVE,
                    )
                {
                    // Render at the panel's real size (reported by the browser
                    // via /api/viewport/resize) so the frame fills the viewport
                    // edge-to-edge like Godot, instead of letterboxing a fixed
                    // 640x480 box into a centered square.
                    let (rw, rh) = (state.render_width.clamp(64, 2048), state.render_height.clamp(64, 2048));
                    // Interactive render scale is a live perf knob (0.1..1.0×).
                    let scale = if camera_lowres {
                        (gdeditor::editor_server::PERF_SCALE_X1000
                            .load(std::sync::atomic::Ordering::Relaxed) as f32)
                            / 1000.0
                    } else {
                        1.0
                    };
                    let this_lowres = scale < 1.0;
                    // Perf knob: optionally skip the ground grid (~200 projected
                    // lines) while the camera moves; it returns on the sharpen frame.
                    let restore_grid = if this_lowres
                        && state.editor_mode == "3d"
                        && !gdeditor::editor_server::PERF_GRID_IN_MOTION
                            .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        let saved = state.editor_viewport_3d.grid.visible;
                        state.editor_viewport_3d.grid.visible = false;
                        Some(saved)
                    } else {
                        None
                    };
                    let job = if state.editor_mode == "3d" {
                        // pat-7i8u4 / pat-3dnav: in 3D mode the viewport shows a
                        // perspective render from the *persistent* editor camera
                        // (so orbit/pan/zoom accumulate), not the 2D rasterizer.
                        // Set the viewport dims to the ACTUAL rendered size (which
                        // may be reduced while orbiting) so 3D picking / gizmo math
                        // stays consistent with the displayed frame's pixels — the
                        // browser scales its click coords by the image's natural
                        // width, which equals these dims.
                        state.editor_viewport_3d.width = ((rw as f32 * scale) as u32).max(64);
                        state.editor_viewport_3d.height = ((rh as f32 * scale) as u32).max(64);
                        // Keep the 3D viewport selection in sync with the editor
                        // selection so the selection outline + transform gizmo
                        // follow tree/inspector selection too — and clear when a
                        // selected node is deleted (pat-3dnav sync fix).
                        let ids: Vec<u64> =
                            state.selected_nodes.iter().map(|id| id.raw()).collect();
                        state.editor_viewport_3d.selection.selected_nodes = ids;
                        // pat-ism62 lock-hold reduction: snapshot only the
                        // tree-dependent inputs (viewport clone + render list) under
                        // the lock; the GPU submit, blocking readback, overlay
                        // rasterization, and BMP encode run AFTER the lock releases,
                        // so orbit/select/gizmo POSTs aren't serialized behind the
                        // full-res render.
                        let vp = state.editor_viewport_3d.clone();
                        let inputs = gdeditor::viewport_3d_render::editor_3d_render_inputs(
                            &vp,
                            &state.scene_tree,
                        );
                        RenderJob::Gpu { vp, inputs }
                    } else {
                        let selected = state.selected_node;
                        let zoom = state.viewport_zoom;
                        let pan = state.viewport_pan;
                        RenderJob::Cpu(gdeditor::scene_renderer::render_scene_with_zoom_pan(
                            &state.scene_tree,
                            selected,
                            rw,
                            rh,
                            zoom,
                            pan,
                        ))
                    };
                    if let Some(saved) = restore_grid {
                        state.editor_viewport_3d.grid.visible = saved;
                    }
                    last_render_sig = Some(sig);
                    Some((job, this_lowres))
                } else {
                    None
                }
            };
            if let Some((job, this_lowres)) = maybe_fb {
                // For the 2D path, stream BMP-only while actively interacting: BMP
                // encodes far cheaper than PNG deflate, so refreshing PNG every
                // frame would cap the rate well under 60fps. The settle frame after
                // motion stops (interaction_active clears) refreshes the PNG too.
                let full_quality = !this_lowres && !interaction_active;
                match job {
                    // GPU 3D path: the render (submit + blocking readback + overlay
                    // compositing) runs HERE, off the state lock, from the snapshot.
                    // Then encode BMP with a cheap per-row copy (no per-pixel Color
                    // pass) and publish. Only BMP is streamed live, so PNG is skipped.
                    RenderJob::Gpu { vp, inputs } => {
                        let raster_t = std::time::Instant::now();
                        let img = gdeditor::viewport_3d_render::render_editor_3d_frame_gpu(&vp, &inputs);
                        gdeditor::editor_server::PERF_LAST_RASTER_US.store(
                            raster_t.elapsed().as_micros().min(u32::MAX as u128) as u32,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        let (w, h) = (img.width, img.height);
                        let enc_t = std::time::Instant::now();
                        let bmp = gdrender2d::encode_bmp_bgra(&img);
                        gdeditor::editor_server::PERF_LAST_ENCODE_US.store(
                            enc_t.elapsed().as_micros().min(u32::MAX as u128) as u32,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        handle.update_frame_bmp(bmp, w, h);
                    }
                    // 2D path: the float FrameBuffer encodes BMP (+ PNG on settle).
                    RenderJob::Cpu(fb) => handle.update_frame_ex(fb, full_quality),
                }
                last_render_at = std::time::Instant::now();
                last_frame_lowres = this_lowres;
                // A frame changed → stay in the fast-poll window for a moment so a
                // continuing drag/orbit renders at interactive rate.
                active_render_until = last_render_at + Duration::from_millis(300);
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
