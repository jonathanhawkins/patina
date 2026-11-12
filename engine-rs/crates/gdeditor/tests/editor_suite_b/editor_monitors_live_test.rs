//! Monitors report live runtime values, not 0 (pat-wnwio).
//!
//! Godot's EditorPerformanceProfiler shows the `Performance` singleton counters
//! (editor_performance_profiler.cpp): render objects/primitives/draw-calls,
//! video/texture/buffer memory, static memory, physics 2D/3D, navigation. This
//! drives the assembled server's `/api/monitors` endpoint and asserts the
//! counters reflect the live scene (drawable objects, navigation regions, node
//! and memory totals) instead of a flat 0.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // A small scene with drawable, physics, and navigation nodes so the
    // render / physics / navigation counters have something to report.
    let world = tree.add_child(root, Node::new("World", "Node2D")).unwrap();
    tree.add_child(world, Node::new("Hero", "Sprite2D"))
        .unwrap();
    tree.add_child(world, Node::new("Crate", "RigidBody2D"))
        .unwrap();
    tree.add_child(world, Node::new("NavMesh", "NavigationRegion2D"))
        .unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn body_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).expect("valid JSON")
}

#[test]
fn monitors_report_live_runtime_values() {
    let (handle, port) = make_server();

    let m = body_json(&http_get(port, "/api/monitors"));
    let mon = &m["monitors"];
    assert!(mon.is_object(), "/api/monitors returns a monitors object");

    let num = |key: &str| mon[key].as_f64().unwrap_or(-1.0);

    // Object counters reflect the live scene (root + 4 nodes).
    assert!(
        num("object/nodes") >= 4.0,
        "object/nodes is live: {}",
        num("object/nodes")
    );
    assert!(
        num("object/objects") >= num("object/nodes"),
        "objects >= nodes"
    );

    // Raster counters were previously a flat 0 — now driven by the drawable
    // nodes (Node2D World + Sprite2D Hero; the body/nav nodes don't draw).
    assert!(
        num("raster/total_objects_drawn") >= 2.0,
        "draw objects live: {}",
        num("raster/total_objects_drawn")
    );
    assert_eq!(
        num("raster/total_draw_calls"),
        num("raster/total_objects_drawn"),
        "one draw call per drawn object"
    );
    assert!(
        num("raster/total_primitives_drawn") >= num("raster/total_objects_drawn"),
        "primitives >= objects"
    );

    // Navigation reports the region we added (was 0).
    assert!(
        num("navigation/regions") >= 1.0,
        "navigation/regions live: {}",
        num("navigation/regions")
    );
    assert_eq!(
        num("navigation/active_maps"),
        1.0,
        "a region implies an active map"
    );

    // Memory + video counters are non-negative real values.
    assert!(num("memory/static") > 0.0, "static memory reported");
    assert!(num("video/video_mem_used") >= 0.0 && mon["video/texture_mem_used"].is_number());

    // Physics 2D body is tracked (active only while stepping, but the category exists).
    assert!(mon["physics_2d/active_objects"].is_number());

    // The served Monitors panel fetches the live counters + renders the tree.
    let html = http_get(port, "/editor");
    for needle in [
        "/api/monitors",
        "function fetchMonitorData",
        "raster/total_draw_calls",
        "navigation/regions",
    ] {
        assert!(html.contains(needle), "Monitors panel missing `{needle}`");
    }

    handle.stop();
}
