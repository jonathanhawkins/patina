//! The 2D view's page overlay (origin lines, grid, rulers, guides) must put
//! world (0,0) where the server draws it. Found importing the official Godot
//! demo "Dodge the Creeps": the rulers' 0 sat 217px from the scene's origin.
//! The server frames the view on the scene's BOUNDS
//! (`scene_renderer::camera_offset`: `width/2 - centre*zoom + pan`) while the
//! page assumed `width/2 + pan`, so every overlay element was off by the
//! bounds centre — and a guide dropped on a node landed somewhere else.
//!
//! The server now reports that centre, and the page uses the same formula.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn get(port: u16, path: &str) -> serde_json::Value {
    let mut stream = (0..40)
        .find_map(|_| {
            TcpStream::connect(format!("127.0.0.1:{port}")).ok().or_else(|| {
                thread::sleep(Duration::from_millis(50));
                None
            })
        })
        .expect("connect");
    write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap()
}

fn scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    // Bounds that are NOT centred on the origin, like Dodge's 480x720 layout.
    for (name, x, y) in [("A", 0.0, 0.0), ("B", 480.0, 720.0)] {
        let mut n = Node::new(name, "Node2D");
        n.set_property(
            "position",
            Variant::Vector2(gdcore::math::Vector2::new(x, y)),
        );
        tree.add_child(main, n).unwrap();
    }
    tree
}

#[test]
fn the_reported_view_origin_is_the_one_the_server_renders_with() {
    let expected = gdeditor::scene_renderer::camera_offset(&scene(), 800, 600);

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, EditorState::new(scene()));
    thread::sleep(Duration::from_millis(100));
    let v = get(port, "/api/viewport/zoom_pan");
    handle.stop();

    let (zoom, pan_x, pan_y) = (
        v["zoom"].as_f64().unwrap() as f32,
        v["pan_x"].as_f64().unwrap() as f32,
        v["pan_y"].as_f64().unwrap() as f32,
    );
    let cx = v["scene_center_x"].as_f64().expect("scene_center_x is reported") as f32;
    let cy = v["scene_center_y"].as_f64().expect("scene_center_y is reported") as f32;
    assert!(cx != 0.0 || cy != 0.0, "precondition: the bounds are off-origin");
    // The page's formula (`viewOriginX/Y`) on the reported numbers lands on
    // the server's own origin.
    let page_x = 800.0 / 2.0 + pan_x - cx * zoom;
    let page_y = 600.0 / 2.0 + pan_y - cy * zoom;
    assert!((page_x - expected.x).abs() < 1e-3, "x: page {page_x} vs server {}", expected.x);
    assert!((page_y - expected.y).abs() < 1e-3, "y: page {page_y} vs server {}", expected.y);
}

#[test]
fn the_page_overlay_uses_the_scene_centred_origin_everywhere() {
    assert!(EDITOR_HTML.contains(
        "function viewOriginX(width) { return width / 2 + viewportPanX - viewportSceneCX * viewportZoom; }"
    ));
    assert!(EDITOR_HTML.contains("var ox = viewOriginX(cw);"));
    // No screen<->world conversion is left on the old panel-centred formula.
    for stale in ["(rect.width / 2 + viewportPanX)", "(rect.height / 2 + viewportPanY)", "var ox = cw / 2 + panX;"] {
        assert!(!EDITOR_HTML.contains(stale), "stale overlay origin: {stale}");
    }
}
