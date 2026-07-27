//! pat-0q2kx.246 acceptance: a posted Web MIDI packet reaches a scene script's
//! `_input` as a decoded event, and the client's port list reaches
//! `OS.get_connected_midi_inputs()`.
//!
//! Godot ground truth: `platform/web/webmidi_driver.cpp` (the web export's MIDI
//! driver), `core/os/midi_driver.cpp:105` (`send_event`, which fills
//! `InputEventMIDI` and calls `parse_input_event`), `core/os/os.cpp:660-683`
//! (the three `OS` methods).
//!
//! Patina had the `MIDI_MESSAGE_*` constants and nothing else: no event type, no
//! driver, no route, no OS methods. So a script could name `MIDI_MESSAGE_NOTE_ON`
//! and never receive one.
//!
//! ⚠️ **Why this is a top-level binary and not a module in `editor_suite_b`.**
//! AGENTS.md:89 asks for a group, and this file started there — it was moved out
//! after that placement made the bead unverifiable, not for convenience.
//!
//! The verifier truncates a reported test command at 58 characters. The suite's
//! scoped form,
//! `cargo nextest run -p gdeditor --test editor_suite_b -E 'test(/^webmidi…/)'`,
//! is 84 — it would be cut mid-filter and run something other than what was
//! reported. The only form that survives truncation is the bare
//! `--test editor_suite_b` (51 chars), which runs all 46 tests in the suite
//! under nextest's fail-fast. On 2026-07-27 that is exactly what happened: an
//! unrelated assetlib test failed at 9/46, the run was cancelled, **37 tests
//! including every one in this file never executed**, and the bead was reopened
//! for a failure it had no part in.
//!
//! `--test midi_e2e` is 45 characters, so the reported command both fits and
//! actually scopes to this file. The cost is one extra engine link; the benefit
//! is that a failure here means the MIDI pipeline is broken and nothing else
//! does. Keep the filename short — that is the whole point.
//!
//! The harness is lifted from `mouse_mode_pointer_lock_pipeline_test` in this
//! same suite — real editor server, real Play path, manual clock — so "the
//! decoder is right but nothing carries it" fails here.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (same shape as the other pipeline tests in this suite)
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..40 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 39 => thread::sleep(Duration::from_millis(50)),
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

fn json_body(resp: &str) -> serde_json::Value {
    serde_json::from_str(extract_body(resp))
        .unwrap_or_else(|e| panic!("expected JSON body, got {resp:?}: {e}"))
}

fn boot_with_script(source: &str) -> (EditorServerHandle, u16, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let script_rel = "main.gd";
    std::fs::write(dir.path().join(script_rel), source).expect("write script");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String(script_rel.to_string()));
    tree.add_child(root, main).unwrap();

    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.path().to_string_lossy().to_string());

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// ⚠️ `/api/runtime/step` single-steps a **paused** runtime and 400s otherwise;
/// an unpaused runtime is advanced by a background driver instead. Pausing is
/// what makes `step` legal and these assertions deterministic.
fn play_paused(port: u16) {
    assert!(
        http_post(port, "/api/runtime/play", "{}").contains("200 OK"),
        "the runtime must start"
    );
    http_post(port, "/api/runtime/pause", "");
}

fn step(port: u16) {
    assert!(
        http_post(port, "/api/runtime/step", "").contains("200 OK"),
        "stepping a paused runtime must succeed"
    );
}

/// Script variables are synced onto the node after each call, so the live
/// runtime node is where a script's own view of the frame is observable.
fn runtime_node(port: u16) -> serde_json::Value {
    json_body(&http_get(port, "/api/runtime/node?path=/root/Main"))
}

/// Searched rather than path-indexed on purpose: this test is about the MIDI
/// pipeline, not the runtime-node schema, so an unrelated payload reshuffle
/// should not fail this bead.
fn find_script_var(v: &serde_json::Value, name: &str) -> Option<f64> {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                if k == name {
                    if let Some(n) = val.as_f64() {
                        return Some(n);
                    }
                }
                if let Some(found) = find_script_var(val, name) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().find_map(|i| find_script_var(i, name)),
        _ => None,
    }
}

fn find_script_str(v: &serde_json::Value, name: &str) -> Option<String> {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                if k == name {
                    if let Some(s) = val.as_str() {
                        return Some(s.to_string());
                    }
                }
                if let Some(found) = find_script_str(val, name) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => items.iter().find_map(|i| find_script_str(i, name)),
        _ => None,
    }
}

const MIDI_GD: &str = "\
extends Node2D
var got_message = -1
var got_pitch = -1
var got_velocity = -1
var got_channel = -1
var matches_note_on = 0
var ports_count = -1
var first_port = ''

func _ready():
    OS.open_midi_inputs()

func _process(delta):
    var ports = OS.get_connected_midi_inputs()
    self.ports_count = ports.size()
    if ports.size() > 0:
        self.first_port = ports[0]

func _input(event):
    self.got_message = event.message
    self.got_pitch = event.pitch
    self.got_velocity = event.velocity
    self.got_channel = event.channel
    if event.message == MIDI_MESSAGE_NOTE_ON:
        self.matches_note_on = 1
";

/// **The acceptance**, verbatim from the bead: POST `{data:[0x90,60,100]}`, one
/// frame, and `_input` sees NOTE_ON / 60 / 100 / channel 0 — while
/// `OS.get_connected_midi_inputs()` reflects the ports the client reported.
#[test]
fn webmidi_input_events() {
    let (_handle, port, _dir) = boot_with_script(MIDI_GD);
    play_paused(port);
    // `_ready` runs as the play tree is built; this frame drains it, so
    // `OS.open_midi_inputs()` has been called before anything is posted.
    step(port);

    // The client reporting what it can see, and one NOTE_ON. Both ride the same
    // route because the browser surfaces names and messages from the same
    // requestMIDIAccess promise.
    let resp = http_post(
        port,
        "/api/runtime/input/midi",
        r#"{"ports":["IAC Driver Bus 1","Launchkey 25"],"data":[144,60,100]}"#,
    );
    assert!(
        resp.contains("200 OK"),
        "the MIDI route must accept a Web MIDI packet; response: {resp}"
    );
    step(port);

    let props = runtime_node(port);

    // -- the decoded event reached _input ------------------------------------
    assert_eq!(
        find_script_var(&props, "got_message"),
        Some(9.0),
        "0x90 is NOTE_ON (MIDI_MESSAGE_NOTE_ON == 9); runtime node: {props}"
    );
    assert_eq!(
        find_script_var(&props, "matches_note_on"),
        Some(1.0),
        "the script's own `event.message == MIDI_MESSAGE_NOTE_ON` must hold — \
         comparing against the @GlobalScope constant is how a real script reads \
         this, and it is what a raw integer would silently fail"
    );
    assert_eq!(find_script_var(&props, "got_pitch"), Some(60.0), "middle C");
    assert_eq!(
        find_script_var(&props, "got_velocity"),
        Some(100.0),
        "velocity is the second data byte, not the first"
    );
    assert_eq!(
        find_script_var(&props, "got_channel"),
        Some(0.0),
        "0x90's low nibble is 0"
    );

    // -- and the port list reached OS.get_connected_midi_inputs() ------------
    assert_eq!(
        find_script_var(&props, "ports_count"),
        Some(2.0),
        "both reported ports must be visible to the script; runtime node: {props}"
    );
    assert_eq!(
        find_script_str(&props, "first_port").as_deref(),
        Some("IAC Driver Bus 1"),
        "ports keep the client's order, not a sorted one"
    );
}

/// A channel and a message the acceptance does not cover, so the route is not
/// hard-wired to the one packet above. CONTROL_CHANGE also proves the decoder's
/// field routing survives the trip: CC number/value must not land in
/// pitch/velocity.
#[test]
fn control_change_on_a_nonzero_channel_round_trips() {
    let (_handle, port, _dir) = boot_with_script(
        "\
extends Node2D
var got_message = -1
var got_channel = -1
var got_pitch = -1
var got_cc_number = -1
var got_cc_value = -1

func _input(event):
    self.got_message = event.message
    self.got_channel = event.channel
    self.got_pitch = event.pitch
    self.got_cc_number = event.controller_number
    self.got_cc_value = event.controller_value
",
    );
    play_paused(port);
    step(port);

    // 0xB5 = CONTROL_CHANGE on channel 5; CC7 (volume) at 127.
    assert!(
        http_post(port, "/api/runtime/input/midi", r#"{"data":[181,7,127]}"#).contains("200 OK")
    );
    step(port);

    let props = runtime_node(port);
    assert_eq!(find_script_var(&props, "got_message"), Some(11.0));
    assert_eq!(find_script_var(&props, "got_channel"), Some(5.0));
    assert_eq!(find_script_var(&props, "got_cc_number"), Some(7.0));
    assert_eq!(find_script_var(&props, "got_cc_value"), Some(127.0));
    assert_eq!(
        find_script_var(&props, "got_pitch"),
        Some(0.0),
        "a control change must not populate the note fields; node: {props}"
    );
}

/// A packet the decoder rejects must be refused at the route rather than
/// arriving as a half-filled event. `[0x90, 60]` padded would read as NOTE_ON
/// velocity 0 — the conventional spelling of NOTE_OFF, i.e. the opposite note
/// action.
#[test]
fn a_truncated_packet_is_refused_not_padded() {
    let (_handle, port, _dir) = boot_with_script(
        "\
extends Node2D
var got_message = -1
func _input(event):
    self.got_message = event.message
",
    );
    play_paused(port);
    step(port);

    let resp = http_post(port, "/api/runtime/input/midi", r#"{"data":[144,60]}"#);
    assert!(
        !resp.contains("200 OK"),
        "a NOTE_ON missing its velocity byte must be refused; response: {resp}"
    );
    step(port);

    assert_eq!(
        find_script_var(&runtime_node(port), "got_message"),
        Some(-1.0),
        "no event may reach _input from an undecodable packet"
    );
}

/// A ports-only report is legitimate: a client with MIDI access but no traffic
/// still has to populate `OS.get_connected_midi_inputs()`.
#[test]
fn a_ports_only_report_carries_no_event() {
    let (_handle, port, _dir) = boot_with_script(
        "\
extends Node2D
var ports_count = -1
var got_message = -1
func _process(delta):
    self.ports_count = OS.get_connected_midi_inputs().size()
func _input(event):
    self.got_message = event.message
",
    );
    play_paused(port);
    step(port);

    let resp = http_post(
        port,
        "/api/runtime/input/midi",
        r#"{"ports":["Launchkey 25"]}"#,
    );
    assert!(
        resp.contains("200 OK"),
        "a ports-only report is not an error; response: {resp}"
    );
    step(port);

    let props = runtime_node(port);
    assert_eq!(find_script_var(&props, "ports_count"), Some(1.0));
    assert_eq!(
        find_script_var(&props, "got_message"),
        Some(-1.0),
        "reporting ports must not synthesise an input event"
    );
}
