//! App-level regression test for pat-7i8u4.
//!
//! The editor's 3D main-screen mode was cosmetic: clicking 3D toggled the
//! button and recorded the mode, but the viewport always rendered the 2D
//! rasterizer. These tests prove the 3D viewport now renders a real perspective
//! frame (grid + spatial-node boxes) from an editor camera, distinct from the
//! 2D-mode frame, and that the server reports a 3D (non-2D) main view.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::{scene_renderer, viewport_3d_render};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::{node3d, SceneTree};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

const W: u32 = 480;
const H: u32 = 360;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A scene with spatial (3D) content so the 3D viewport has nodes to draw.
fn build_3d_scene() -> SceneTree {
    gdobject::class_db::register_3d_classes();
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let crate_id = tree
        .add_child(root, Node::new("Crate", "MeshInstance3D"))
        .unwrap();
    node3d::set_position(
        &mut tree,
        crate_id,
        gdcore::math::Vector3::new(1.0, 0.5, -2.0),
    );
    tree.add_child(root, Node::new("Cam", "Camera3D")).unwrap();
    tree.add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    tree
}

/// Number of distinct pixel colors in a frame (a flat/empty frame has 1).
fn distinct_colors(fb: &FrameBuffer) -> usize {
    let mut set = HashSet::new();
    for p in &fb.pixels {
        set.insert((
            (p.r * 255.0) as u8,
            (p.g * 255.0) as u8,
            (p.b * 255.0) as u8,
        ));
        if set.len() > 8 {
            break;
        }
    }
    set.len()
}

fn http_request(port: u16, req: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// The 3D viewport renders real geometry from an editor camera, and that frame
/// is distinct from the 2D-mode render of the same scene — so the mode switch
/// changes the actual view, not just a JSON descriptor.
#[test]
fn editor_3d_viewport_renders_spatial_scene() {
    let tree = build_3d_scene();

    let fb_2d = scene_renderer::render_scene_with_zoom_pan(&tree, None, W, H, 1.0, (0.0, 0.0));
    let fb_3d = viewport_3d_render::render_editor_3d(&tree, W, H);

    assert_eq!(fb_3d.width, W);
    assert_eq!(fb_3d.height, H);

    // The 3D frame must contain rendered geometry (grid lines + node boxes),
    // not a single flat fill.
    assert!(
        distinct_colors(&fb_3d) >= 2,
        "3D viewport frame is a uniform fill — nothing was rendered"
    );

    // It must differ from the 2D render of the same scene: proves the viewport
    // re-renders per mode instead of reusing the 2D rasterizer (the bug).
    assert_eq!(fb_2d.pixels.len(), fb_3d.pixels.len());
    assert!(
        fb_3d.pixels != fb_2d.pixels,
        "3D-mode frame is byte-identical to the 2D-mode frame (mode switch is cosmetic)"
    );
}

/// The running server records 3D mode and reports a non-2D main view, and the
/// 3D frame is served over the viewport endpoint.
#[test]
fn editor_3d_viewport_served_over_http() {
    let tree = build_3d_scene();
    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // Switch to 3D and confirm the server's main view is the spatial editor.
    let _ = http_post(port, "/api/editor/mode", r#"{"mode":"3d"}"#);
    let main_view = http_get(port, "/api/editor/main_view");
    assert!(
        main_view.contains("\"is_2d_viewport\":false") || main_view.contains("spatial"),
        "main_view should report a 3D/spatial view after switching to 3d; got: {main_view}"
    );

    // Render the 3D frame the way the editor loop does and push it; the viewport
    // endpoint must then serve a non-empty PNG.
    let fb_3d = {
        let st = handle.state().lock().unwrap();
        viewport_3d_render::render_editor_3d(&st.scene_tree, W, H)
    };
    handle.update_frame(fb_3d);

    let resp = http_get(port, "/api/viewport/png");
    assert!(
        resp.starts_with("HTTP/1.1 200"),
        "GET /api/viewport/png should return 200 in 3D mode; got: {}",
        resp.lines().next().unwrap_or("")
    );
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(
        !body.is_empty(),
        "viewport PNG body should be non-empty in 3D mode"
    );

    handle.stop();
}

/// Binary-safe HTTP GET returning just the response body bytes (the String helper
/// would corrupt PNG/BMP bytes via lossy UTF-8).
fn http_get_body_bytes(port: u16, path: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    match buf.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(pos) => buf[pos + 4..].to_vec(),
        None => Vec::new(),
    }
}

/// The interactive fast-stream path (the 60fps optimization): the live 3D viewport
/// streams BMP (`/api/viewport`), which is far cheaper to encode than a deflated
/// PNG. `update_frame_ex(fb, true)` refreshes BOTH the BMP and PNG caches (full-res
/// / stopped / running-game frames), while `update_frame_ex(fb, false)` — a low-res
/// in-motion frame — refreshes ONLY the BMP so the render loop isn't taxed by a PNG
/// nobody fetches, leaving the PNG cache valid for non-motion consumers.
#[test]
fn editor_update_frame_ex_streams_bmp_keeps_png_on_full_quality() {
    let tree = build_3d_scene();
    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // Two frames with different byte contents (different sizes → different pixels).
    let (fb_a, fb_b) = {
        let st = handle.state().lock().unwrap();
        (
            viewport_3d_render::render_editor_3d(&st.scene_tree, W, H),
            viewport_3d_render::render_editor_3d(&st.scene_tree, W / 2, H / 2),
        )
    };

    // Full-quality frame A → both caches populated with valid images.
    handle.update_frame_ex(fb_a, true);
    let png_a = http_get_body_bytes(port, "/api/viewport/png");
    let bmp_a = http_get_body_bytes(port, "/api/viewport");
    assert!(
        png_a.starts_with(&[137, 80, 78, 71]),
        "PNG signature after full-quality frame"
    );
    assert!(
        bmp_a.starts_with(b"BM"),
        "BMP signature after full-quality frame"
    );

    // Low-res in-motion frame B → BMP updates; PNG cache stays the frame-A PNG.
    handle.update_frame_ex(fb_b, false);
    let png_b = http_get_body_bytes(port, "/api/viewport/png");
    let bmp_b = http_get_body_bytes(port, "/api/viewport");
    assert_eq!(
        png_b, png_a,
        "PNG cache must stay put during a low-res in-motion frame"
    );
    assert_ne!(bmp_b, bmp_a, "BMP live stream must update to the new frame");
    assert!(
        bmp_b.starts_with(b"BM"),
        "BMP still valid after the in-motion frame"
    );

    handle.stop();
}

fn json_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// pat-jrayq: 3D viewport phase 2 — orbit/pan/zoom mutate the persisted camera,
/// a MeshInstance3D renders real geometry distinct from the 2D render, the 3D
/// viewport picks a node, and the 2D grid/ruler overlay is hidden in 3D mode.
#[test]
fn editor_3d_viewport_navigation_and_meshes() {
    let tree = build_3d_scene();
    let state = EditorState::new(tree);
    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // --- Persisted camera: orbit/zoom mutate it, reset restores ---
    let s0 = json_body(&http_get(port, "/api/viewport/3d/state")).to_string();
    let after_orbit = json_body(&http_post(
        port,
        "/api/viewport/3d/orbit",
        r#"{"dx":60,"dy":25}"#,
    ))
    .to_string();
    assert_ne!(after_orbit, s0, "orbit must change the persisted 3D camera");
    let s_orbit = json_body(&http_get(port, "/api/viewport/3d/state")).to_string();
    assert_ne!(s_orbit, s0, "orbit persists across a fresh state read");
    let _ = http_post(port, "/api/viewport/3d/zoom", r#"{"delta":120}"#);
    let s_zoom = json_body(&http_get(port, "/api/viewport/3d/state")).to_string();
    assert_ne!(s_zoom, s_orbit, "zoom must change the camera distance");
    let _ = http_post(port, "/api/viewport/3d/reset", "{}");
    let s_reset = json_body(&http_get(port, "/api/viewport/3d/state")).to_string();
    assert_eq!(s_reset, s0, "reset restores the default camera framing");

    // --- MeshInstance3D renders real geometry, distinct from the 2D render ---
    let fb_3d = {
        let st = handle.state().lock().unwrap();
        viewport_3d_render::render_editor_3d(&st.scene_tree, W, H)
    };
    let fb_2d = {
        let st = handle.state().lock().unwrap();
        scene_renderer::render_scene_with_zoom_pan(&st.scene_tree, None, W, H, 1.0, (0.0, 0.0))
    };
    assert!(
        distinct_colors(&fb_3d) >= 2,
        "the MeshInstance3D scene renders geometry in 3D"
    );
    assert!(
        fb_3d.pixels != fb_2d.pixels,
        "the 3D mesh render differs from the 2D render"
    );

    // --- The 3D viewport picks a node (box-select over the whole frame) ---
    let pick_resp = http_post(
        port,
        "/api/viewport/3d/box_select",
        &format!(r#"{{"x1":0,"y1":0,"x2":{W},"y2":{H}}}"#),
    );
    let picked: serde_json::Value =
        serde_json::from_str(json_body(&pick_resp)).expect("box_select returns JSON");
    assert!(
        !picked["selected_nodes"].as_array().unwrap().is_empty(),
        "box-select over the frame picks the 3D node(s)"
    );

    // --- 2D ruler/grid overlay is hidden in 3D mode ---
    let html = http_get(port, "/editor");
    assert!(
        html.contains("if (editorMode === '3d') {") && html.contains("draw3DNavGizmo"),
        "renderViewportOverlay must skip the 2D grid/ruler overlay (only the nav gizmo) in 3D mode"
    );

    handle.stop();
}
