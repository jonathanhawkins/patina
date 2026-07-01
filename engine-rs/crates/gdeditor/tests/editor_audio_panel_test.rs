//! App-level integration tests for the bottom-panel **Audio bus layout editor**
//! (Godot `EditorAudioBuses` parity).
//!
//! Boots the real `EditorServerHandle` over HTTP and exercises the full audio
//! bus API surface the Audio tab drives — add/remove/rename bus, volume,
//! mute/solo/bypass, effect add/toggle/remove, output routing, and load-default
//! — then asserts the served editor HTML actually contains the Audio panel so
//! the tab is wired end to end (not just the backend).

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
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
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

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Parses the JSON body of an audio response, asserting HTTP 200 first.
fn audio_json(resp: &str) -> serde_json::Value {
    assert!(resp.contains("200 OK"), "expected 200, got:\n{resp}");
    serde_json::from_str(extract_body(resp)).expect("audio response is valid JSON")
}

fn buses(v: &serde_json::Value) -> &Vec<serde_json::Value> {
    v["buses"].as_array().expect("buses array")
}

#[test]
fn audio_layout_starts_with_master_and_effect_catalog() {
    let (handle, port) = make_server();

    let v = audio_json(&http_get(port, "/api/audio/buses"));
    assert_eq!(v["ok"], serde_json::json!(true));
    let bs = buses(&v);
    assert_eq!(bs.len(), 1, "layout starts with the lone Master bus");
    assert_eq!(bs[0]["name"], "Master");
    assert_eq!(bs[0]["is_master"], serde_json::json!(true));
    assert_eq!(bs[0]["volume_db"], serde_json::json!(0.0));

    // The Add Effect dropdown is populated from a non-empty catalog.
    let fx = v["effect_types"].as_array().expect("effect_types array");
    assert!(fx.len() >= 10, "effect catalog should be substantial");
    assert!(fx.iter().any(|e| e == "Reverb"));

    handle.stop();
}

#[test]
fn audio_add_remove_rename_buses() {
    let (handle, port) = make_server();

    // Add a bus — defaults to routing into Master.
    let v = audio_json(&http_post(port, "/api/audio/bus/add", "{\"name\":\"Music\"}"));
    let bs = buses(&v);
    assert_eq!(bs.len(), 2);
    assert_eq!(bs[1]["name"], "Music");
    assert_eq!(bs[1]["is_master"], serde_json::json!(false));
    assert_eq!(bs[1]["output"], "Master");

    // Rename it.
    let v = audio_json(&http_post(
        port,
        "/api/audio/bus/rename",
        "{\"index\":1,\"name\":\"BGM\"}",
    ));
    assert_eq!(buses(&v)[1]["name"], "BGM");

    // Master (index 0) is fixed: rename is rejected.
    let v = audio_json(&http_post(
        port,
        "/api/audio/bus/rename",
        "{\"index\":0,\"name\":\"Nope\"}",
    ));
    assert_eq!(buses(&v)[0]["name"], "Master");

    // Remove the bus.
    let v = audio_json(&http_post(port, "/api/audio/bus/remove", "{\"index\":1}"));
    assert_eq!(buses(&v).len(), 1);

    // Master cannot be removed.
    let v = audio_json(&http_post(port, "/api/audio/bus/remove", "{\"index\":0}"));
    assert_eq!(buses(&v).len(), 1);

    handle.stop();
}

#[test]
fn audio_volume_mute_solo_bypass() {
    let (handle, port) = make_server();
    http_post(port, "/api/audio/bus/add", "{\"name\":\"SFX\"}");

    let v = audio_json(&http_post(
        port,
        "/api/audio/bus/volume",
        "{\"index\":1,\"volume_db\":-6.0}",
    ));
    assert_eq!(buses(&v)[1]["volume_db"], serde_json::json!(-6.0));

    let v = audio_json(&http_post(port, "/api/audio/bus/mute", "{\"index\":1}"));
    assert_eq!(buses(&v)[1]["muted"], serde_json::json!(true));
    let v = audio_json(&http_post(port, "/api/audio/bus/mute", "{\"index\":1}"));
    assert_eq!(buses(&v)[1]["muted"], serde_json::json!(false));

    let v = audio_json(&http_post(port, "/api/audio/bus/solo", "{\"index\":1}"));
    assert_eq!(buses(&v)[1]["soloed"], serde_json::json!(true));

    let v = audio_json(&http_post(port, "/api/audio/bus/bypass", "{\"index\":1}"));
    assert_eq!(buses(&v)[1]["bypassed"], serde_json::json!(true));

    handle.stop();
}

#[test]
fn audio_effects_add_toggle_remove() {
    let (handle, port) = make_server();

    // Add an effect to Master.
    let v = audio_json(&http_post(
        port,
        "/api/audio/effect/add",
        "{\"index\":0,\"name\":\"Reverb\"}",
    ));
    let fx = buses(&v)[0]["effects"].as_array().unwrap();
    assert_eq!(fx.len(), 1);
    assert_eq!(fx[0]["name"], "Reverb");
    assert_eq!(fx[0]["enabled"], serde_json::json!(true));

    // Toggle it off.
    let v = audio_json(&http_post(
        port,
        "/api/audio/effect/toggle",
        "{\"index\":0,\"effect_index\":0}",
    ));
    assert_eq!(
        buses(&v)[0]["effects"][0]["enabled"],
        serde_json::json!(false)
    );

    // Remove it.
    let v = audio_json(&http_post(
        port,
        "/api/audio/effect/remove",
        "{\"index\":0,\"effect_index\":0}",
    ));
    assert_eq!(buses(&v)[0]["effects"].as_array().unwrap().len(), 0);

    handle.stop();
}

#[test]
fn audio_output_routing_and_load_default() {
    let (handle, port) = make_server();
    http_post(port, "/api/audio/bus/add", "{\"name\":\"Music\"}"); // index 1
    http_post(port, "/api/audio/bus/add", "{\"name\":\"SFX\"}"); // index 2

    // Route Music (1) into SFX (2) instead of Master.
    let v = audio_json(&http_post(
        port,
        "/api/audio/bus/output",
        "{\"index\":1,\"target\":\"SFX\"}",
    ));
    assert_eq!(buses(&v)[1]["output"], "SFX");

    // Load Default collapses back to a lone Master bus.
    let v = audio_json(&http_post(port, "/api/audio/layout/load_default", "{}"));
    let bs = buses(&v);
    assert_eq!(bs.len(), 1);
    assert_eq!(bs[0]["name"], "Master");

    handle.stop();
}

#[test]
fn editor_html_contains_audio_panel() {
    let (handle, port) = make_server();
    let html = http_get(port, "/editor");
    let b = extract_body(&html);

    // The Audio bottom tab and its panel scaffold are present.
    assert!(b.contains("data-tab=\"audio\""), "missing Audio bottom tab");
    assert!(b.contains("id=\"audio-buses\""), "missing audio bus strip host");
    assert!(b.contains("id=\"audio-add-bus\""), "missing Add Bus button");
    assert!(b.contains("Load Default"), "missing Load Default button");
    // The audio panel wiring function is shipped to the client.
    assert!(
        b.contains("setupAudioPanel"),
        "audio panel JS not wired into the editor client"
    );
    // 1:1 Godot bus-strip details: dB scale beside the fader, the Master's fixed
    // "Speakers" output, and the per-bus delete (⋮) control.
    assert!(b.contains("abus-db-scale"), "missing fader dB scale");
    assert!(b.contains("Speakers"), "missing Master 'Speakers' output");
    assert!(b.contains("bus-remove"), "missing per-bus delete control");
    assert!(b.contains("Add Effect"), "missing per-bus Add Effect dropdown");

    handle.stop();
}
