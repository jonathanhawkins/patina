//! The agent interface (`POST /mcp`) driven end to end over HTTP, the way an AI
//! client uses it: open a project, build a scene through the `edit` batch, save
//! it, then `play` it headless against an input timeline and read back the
//! frame-tagged prints, errors, probes and screenshots.
//!
//! Several of these are also regressions for editor bugs the interface exposed
//! on first use — each was reachable from the browser editor too:
//! * a script attached by path (the Create Script dialog's write) never ran in
//!   Play and saved as `script = "res://x.gd"`, which Godot rejects;
//! * a "New RectangleShape2D" could not be sized (`/api/resource/property/set`
//!   refused the inline marker) and collided as nothing;
//! * a Control's `size`/`position` were undone by the next reflow and saved as 0;
//! * nothing drained a played scene's `print()` or runtime errors.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;

/// Serializes the tests in this binary that touch the res:// path roots (every /api/project/open calls set_path_roots) (pat-hukbz).
///
/// That state is process-global. Nextest gives each test its own process, but
/// plain `cargo test` runs the tests as threads in ONE process, where they
/// overwrote each other's state mid-test. Bind the guard for the whole test.
/// Poison-tolerant, so one failing test does not fail the rest.
#[must_use = "bind it for the whole test: `let _guard = serial();`"]
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

const PROJECT: &str = r#"config_version=5

[application]
config/name="AgentTest"
run/main_scene="res://main.tscn"

[input]
move_right={"deadzone": 0.5, "events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":0,"physical_keycode":68,"key_label":0,"unicode":100,"location":0,"echo":false,"script":null)]}
"#;

const PLAYER_GD: &str = "extends CharacterBody2D\n\nvar presses := 0\n\nfunc _ready():\n\tprint(\"ready\")\n\nfunc _physics_process(delta):\n\tvelocity.x = Input.get_axis(\"ui_left\", \"move_right\") * 200.0\n\tif Input.is_action_just_pressed(\"ui_accept\"):\n\t\tpresses += 1\n\tmove_and_slide()\n";

struct Server {
    handle: EditorServerHandle,
    dir: tempfile::TempDir,
}

impl Server {
    fn port(&self) -> u16 {
        self.handle.port()
    }
    fn path(&self, rel: &str) -> std::path::PathBuf {
        self.dir.path().join(rel)
    }
}

/// A project on disk with an empty `main.tscn`, opened in a fresh editor.
fn boot(auth: Option<&str>) -> Server {
    boot_with_project(auth, PROJECT)
}

/// [`boot`] with a project.godot of the test's own.
fn boot_with_project(auth: Option<&str>, project_godot: &str) -> Server {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.godot"), project_godot).unwrap();
    std::fs::write(
        dir.path().join("main.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("player.gd"), PLAYER_GD).unwrap();
    let state = EditorState::new(SceneTree::new());
    let handle = match auth {
        Some(t) => EditorServerHandle::start_with_auth(0, state, Some(t.to_string())),
        None => EditorServerHandle::start(0, state),
    };
    let server = Server { handle, dir };
    let root = server.dir.path().display().to_string();
    let (status, _) = http(
        server.port(),
        "POST",
        "/api/project/open",
        &json!({"path": root}).to_string(),
        auth,
        None,
    );
    assert_eq!(status, 200, "project/open");
    let opened = tool(
        &server,
        "scene",
        json!({"action": "open", "path": "res://main.tscn"}),
        auth,
    );
    assert!(!opened.error, "{}", opened.text);
    server
}

fn http(
    port: u16,
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
    agent_key: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    if let Some(k) = agent_key {
        req.push_str(&format!("X-Patina-Agent: {k}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn rpc(port: u16, method: &str, params: Value, token: Option<&str>) -> (u16, Value) {
    let body = json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params}).to_string();
    let (status, text) = http(port, "POST", "/mcp", &body, token, None);
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

struct ToolResult {
    error: bool,
    text: String,
    images: Vec<Vec<u8>>,
}

fn tool(server: &Server, name: &str, args: Value, token: Option<&str>) -> ToolResult {
    let (status, reply) = rpc(
        server.port(),
        "tools/call",
        json!({"name": name, "arguments": args}),
        token,
    );
    assert_eq!(status, 200, "tools/call {name}: {reply}");
    let result = &reply["result"];
    let mut text = String::new();
    let mut images = Vec::new();
    for block in result["content"].as_array().expect("content array") {
        match block["type"].as_str() {
            Some("text") => text.push_str(block["text"].as_str().unwrap()),
            Some("image") => {
                assert_eq!(block["mimeType"], "image/png");
                images.push(base64_decode(block["data"].as_str().unwrap()));
            }
            other => panic!("unexpected content block {other:?}"),
        }
    }
    ToolResult {
        error: result["isError"].as_bool().unwrap_or(true),
        text,
        images,
    }
}

fn base64_decode(s: &str) -> Vec<u8> {
    let table = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, b) in chunk.iter().enumerate() {
            n |= table(*b) << (18 - 6 * i);
        }
        for i in 0..chunk.len().saturating_sub(1) {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

fn build_player(server: &Server) -> ToolResult {
    tool(
        server,
        "edit",
        json!({"ops": [
            {"op": "add", "type": "CharacterBody2D", "name": "Player", "props": {"position": "Vector2(100, 200)"}},
            {"op": "add", "type": "ColorRect", "name": "Body", "parent": "Player",
             "props": {"size": "Vector2(32, 32)", "position": "Vector2(-16, -16)", "color": "Color(0.2, 0.8, 0.3, 1)"}},
            {"op": "add", "type": "CollisionShape2D", "name": "Shape", "parent": "Player",
             "props": {"shape": {"new": "RectangleShape2D", "size": "Vector2(32, 32)"}}},
            {"op": "script", "node": "Player", "path": "res://player.gd"},
            {"op": "add", "type": "Label", "name": "Hud", "props": {"text": "1.5"}}
        ]}),
        None,
    )
}

#[test]
fn mcp_handshake_lists_tools_and_answers_notifications() {
    let _guard = serial();
    let server = boot(None);
    let (status, init) = rpc(
        server.port(),
        "initialize",
        json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}),
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(
        init["result"]["protocolVersion"], "2025-03-26",
        "a supported revision is echoed"
    );
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("Godot literal"));

    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
    let (status, body) = http(server.port(), "POST", "/mcp", &note, None, None);
    assert_eq!(status, 202, "a notification gets 202 and no body");
    assert!(body.is_empty());

    let (_, list) = rpc(server.port(), "tools/list", json!({}), None);
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "status",
        "tree",
        "inspect",
        "edit",
        "scene",
        "write_script",
        "play",
        "step",
        "audio",
        "stop",
        "screenshot",
        "log",
        "class_info",
        "api",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}: {names:?}"
        );
    }
    for t in list["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
    }

    let (_, unknown) = rpc(server.port(), "no/such", json!({}), None);
    assert_eq!(unknown["error"]["code"], -32601);
    let (_, bad) = {
        let (s, t) = http(server.port(), "POST", "/mcp", "{not json", None, None);
        (s, serde_json::from_str::<Value>(&t).unwrap())
    };
    assert_eq!(bad["error"]["code"], -32700);
    let (status, _) = http(server.port(), "GET", "/mcp", "", None, None);
    assert_eq!(status, 405, "no server-initiated stream is offered");
}

#[test]
fn mcp_sits_behind_the_editor_token() {
    let _guard = serial();
    let server = boot(Some("s3cret"));
    let (status, _) = rpc(server.port(), "tools/list", json!({}), None);
    assert_eq!(
        status, 401,
        "/mcp drives everything /api does, so it needs the same token"
    );
    let (status, _) = rpc(server.port(), "tools/list", json!({}), Some("s3cret"));
    assert_eq!(status, 200);
    // The loopback calls a tool makes must carry the token too.
    let status_tool = tool(&server, "status", json!({}), Some("s3cret"));
    assert!(!status_tool.error, "{}", status_tool.text);
    let built = tool(
        &server,
        "edit",
        json!({"ops": [{"op": "add", "type": "Node2D", "name": "Guarded"}]}),
        Some("s3cret"),
    );
    assert!(!built.error, "{}", built.text);
}

#[test]
fn edit_then_save_writes_a_scene_godot_can_load() {
    let _guard = serial();
    let server = boot(None);
    let built = build_player(&server);
    assert!(!built.error, "{}", built.text);
    assert!(
        built
            .text
            .contains("Player (CharacterBody2D) [res://player.gd]"),
        "{}",
        built.text
    );

    let saved = tool(&server, "scene", json!({"action": "save"}), None);
    assert!(!saved.error, "{}", saved.text);
    let tscn = std::fs::read_to_string(server.path("main.tscn")).unwrap();

    // Script: a real ext_resource, not a bare path.
    assert!(
        tscn.contains("[ext_resource type=\"Script\" path=\"res://player.gd\""),
        "{tscn}"
    );
    assert!(tscn.contains("script = ExtResource("), "{tscn}");
    assert!(
        !tscn.contains("script = \"res://"),
        "bare-path script is not valid Godot: {tscn}"
    );
    // Shape: the sub-resource carries the size set after `new`.
    assert!(
        tscn.contains("[sub_resource type=\"RectangleShape2D\""),
        "{tscn}"
    );
    assert!(tscn.contains("size = Vector2(32, 32)"), "{tscn}");
    // Control: offsets reflect BOTH writes — size first, then position.
    for line in [
        "offset_left = -16.0",
        "offset_top = -16.0",
        "offset_right = 16.0",
        "offset_bottom = 16.0",
    ] {
        assert!(tscn.contains(line), "missing `{line}`: {tscn}");
    }
    // A string property keeps text that looks like a number.
    assert!(tscn.contains("text = \"1.5\""), "{tscn}");

    // The physics layer reads the shape from `__shape__*`; a fresh resource
    // must carry its class there or it collides as nothing.
    let state = server.handle.state().lock().unwrap();
    let shape = state
        .scene_tree
        .get_node_by_path("/root/Main/Player/Shape")
        .expect("shape node");
    let node = state.scene_tree.get_node(shape).unwrap();
    assert_eq!(
        node.get_property("__shape__class"),
        gdvariant::Variant::String("RectangleShape2D".into())
    );
}

#[test]
fn play_drives_an_action_and_reports_frame_tagged_output() {
    let _guard = serial();
    let server = boot(None);
    assert!(!build_player(&server).error);
    let report = tool(
        &server,
        "play",
        json!({
            "frames": 30,
            "inputs": [{"action": "move_right", "at": 1, "frames": 30}, {"action": "ui_accept", "at": 5}],
            "probes": [{"node": "Player", "props": ["position", "presses"], "at": 30}]
        }),
        None,
    );
    assert!(!report.error, "{}", report.text);
    assert!(
        report.text.contains("f0 ready"),
        "_ready's print is frame 0: {}",
        report.text
    );
    // 200 px/s for 30 frames at 60 fps is exactly +100 px.
    let probe = report
        .text
        .lines()
        .find(|l| l.contains("f30 Player:"))
        .unwrap_or_else(|| panic!("no f30 probe: {}", report.text));
    let x: f64 = probe
        .split("Vector2(")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("unparseable probe: {probe}"));
    assert!(
        (x - 200.0).abs() < 0.01,
        "expected x≈200 after 30 frames held: {probe}"
    );
    assert!(
        probe.contains("presses=1"),
        "one tap is one just_pressed: {probe}"
    );
    assert!(report.text.contains("game stopped"), "{}", report.text);

    // The same print reached the Output panel's log as game stdout.
    let (_, logs) = http(server.port(), "GET", "/api/logs", "", None, None);
    let logs: Value = serde_json::from_str(&logs).unwrap();
    assert!(
        logs.as_array()
            .unwrap()
            .iter()
            .any(|e| e["category"] == "std" && e["message"] == "ready"),
        "Play's print must reach /api/logs as `std`: {logs}"
    );
}

#[test]
fn unknown_actions_are_rejected_with_the_declared_list() {
    let _guard = serial();
    let server = boot(None);
    let report = tool(
        &server,
        "play",
        json!({"frames": 5, "inputs": [{"action": "jmup", "at": 1}]}),
        None,
    );
    assert!(report.error);
    assert!(
        report.text.contains("unknown action `jmup`") && report.text.contains("move_right"),
        "{}",
        report.text
    );
}

#[test]
fn script_errors_reach_the_report_and_the_errors_tab() {
    let _guard = serial();
    let server = boot(None);
    let written = tool(
        &server,
        "write_script",
        json!({
            "path": "res://bad.gd",
            "source": "extends Node\n\nvar t := 0\n\nfunc _process(delta):\n\tt += 1\n\tif t == 2:\n\t\tpush_error(\"two\")\n\tif t == 3:\n\t\tself.no_such_method()\n",
            "attach_to": "."
        }),
        None,
    );
    assert!(
        !written.error && written.text.contains("no errors"),
        "{}",
        written.text
    );
    // `res://` is the opened project, not the test process's cwd.
    assert!(
        server.path("bad.gd").exists(),
        "script/save must write under the project root"
    );
    let report = tool(&server, "play", json!({"frames": 4}), None);
    assert!(!report.error, "{}", report.text);
    assert!(report.text.contains("f2 ERROR two"), "{}", report.text);
    assert!(
        report
            .text
            .contains("f3 ERROR /root/Main: _process(): method not found: 'no_such_method'"),
        "{}",
        report.text
    );
    let (_, errors) = http(server.port(), "GET", "/api/debug/errors", "", None, None);
    assert!(
        errors.contains("no_such_method"),
        "the Errors tab sees it too: {errors}"
    );
}

#[test]
fn write_script_reports_syntax_errors_with_a_location() {
    let _guard = serial();
    let server = boot(None);
    let written = tool(
        &server,
        "write_script",
        json!({"path": "res://broken.gd", "source": "extends Node\nfunc _ready(:\n\tpass\n"}),
        None,
    );
    assert!(
        !written.error,
        "the file is still written: {}",
        written.text
    );
    assert!(written.text.contains("line 2"), "{}", written.text);
    assert!(server.path("broken.gd").exists());
}

#[test]
fn screenshots_are_png_images_of_the_game_view() {
    let _guard = serial();
    let server = boot(None);
    assert!(!build_player(&server).error);
    let report = tool(
        &server,
        "play",
        json!({"frames": 2, "screenshots": [0, 2], "width": 160, "height": 120}),
        None,
    );
    assert!(!report.error, "{}", report.text);
    assert_eq!(report.images.len(), 2, "{}", report.text);
    for png in &report.images {
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR width/height, big-endian, at bytes 16..24.
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 160);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 120);
    }
}

/// Decodes a PNG to 8-bit RGBA bytes.
fn png_rgba(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let tex = gdrender2d::texture::decode_png(png).expect("a decodable PNG");
    let px = tex
        .pixels
        .iter()
        .flat_map(|c| [c.r, c.g, c.b, c.a].map(|v| (v * 255.0).round() as u8))
        .collect();
    (tex.width, tex.height, px)
}

/// A 3D game had no frame-exact picture: `play` refused every screenshot of a
/// scene with 3D content ("not wired into the agent interface yet"), so a
/// Godot reference frame (pat-xggzs) had nothing to be compared with. The shot
/// must be the Play view's frame: through the scene's Camera3D, so the red box
/// it looks at fills the centre while the corners stay the clear colour.
#[test]
fn play_screenshots_a_3d_scene_through_its_camera() {
    let _guard = serial();
    let server = boot(None);
    std::fs::write(
        server.path("box3d.tscn"),
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="StandardMaterial3D" id="mat"]
shading_mode = 0
albedo_color = Color(1, 0, 0, 1)

[sub_resource type="BoxMesh" id="box"]
material = SubResource("mat")

[node name="World" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
mesh = SubResource("box")

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 3)
current = true
"#,
    )
    .unwrap();
    let opened = tool(
        &server,
        "scene",
        json!({"action": "open", "path": "res://box3d.tscn"}),
        None,
    );
    assert!(!opened.error, "{}", opened.text);
    let report = tool(
        &server,
        "play",
        json!({"frames": 1, "screenshots": [1], "width": 64, "height": 48}),
        None,
    );
    assert!(!report.error, "{}", report.text);
    assert!(
        !report.text.contains("screenshot failed"),
        "{}",
        report.text
    );
    assert_eq!(report.images.len(), 1, "{}", report.text);
    let (w, h, px) = png_rgba(&report.images[0]);
    assert_eq!((w, h), (64, 48));
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2]]
    };
    let centre = at(32, 24);
    assert!(
        centre[0] > 200 && centre[1] < 60 && centre[2] < 60,
        "the unshaded red box should fill the centre, got {centre:?}"
    );
    let corner = at(1, 1);
    assert!(
        !(corner[0] > 200 && corner[1] < 60),
        "the corner is the clear colour, not the box: {corner:?}"
    );
}

#[test]
fn keep_running_then_step_continues_the_same_run_and_edits_are_refused_meanwhile() {
    let _guard = serial();
    let server = boot(None);
    assert!(!build_player(&server).error);
    let first = tool(
        &server,
        "play",
        json!({"frames": 10, "keep_running": true, "inputs": [{"action": "move_right", "at": 1, "frames": 10}]}),
        None,
    );
    assert!(
        !first.error && first.text.contains("left paused"),
        "{}",
        first.text
    );
    let refused = tool(
        &server,
        "edit",
        json!({"ops": [{"op": "add", "type": "Node", "name": "X"}]}),
        None,
    );
    assert!(
        refused.error && refused.text.contains("stop"),
        "{}",
        refused.text
    );
    let more = tool(
        &server,
        "step",
        json!({"frames": 5, "probes": [{"node": "Player", "props": ["position"]}]}),
        None,
    );
    assert!(!more.error, "{}", more.text);
    assert!(
        more.text.contains("f10→f15"),
        "frames continue from the paused run: {}",
        more.text
    );
    let live = tool(&server, "tree", json!({"running": true}), None);
    assert!(
        live.text.contains("Player (CharacterBody2D)"),
        "{}",
        live.text
    );
    assert!(!tool(&server, "stop", json!({}), None).error);
    assert!(
        !tool(
            &server,
            "edit",
            json!({"ops": [{"op": "add", "type": "Node", "name": "X"}]}),
            None
        )
        .error
    );
}

/// Stress: one batch of 120 edits must not trip the per-client rate limiter
/// (60/s) that protects the browser, while an outside client claiming the
/// agent header with the wrong key gets limited as before.
#[test]
fn batch_edits_bypass_the_rate_limiter_but_forged_agent_keys_do_not() {
    let _guard = serial();
    let server = boot(None);
    let mut ops = vec![json!({"op": "add", "type": "Node2D", "name": "Mover"})];
    for i in 0..120 {
        ops.push(json!({"op": "set", "node": "Mover", "props": {"position": format!("Vector2({i}, 0)")}}));
    }
    let batch = tool(
        &server,
        "edit",
        json!({"ops": ops, "show_tree": false}),
        None,
    );
    assert!(!batch.error, "{}", batch.text);
    assert_eq!(batch.text.matches('✓').count(), 121, "{}", batch.text);

    let mut limited = false;
    for _ in 0..120 {
        let (status, _) = http(server.port(), "GET", "/api/scene", "", None, Some("forged"));
        if status == 429 {
            limited = true;
            break;
        }
    }
    assert!(
        limited,
        "a forged X-Patina-Agent key must not exempt a client from the limiter"
    );
}

#[test]
fn scene_new_creates_and_opens_a_file() {
    let _guard = serial();
    let server = boot(None);
    let made = tool(
        &server,
        "scene",
        json!({"action": "new", "path": "res://levels/level_one.tscn", "root_type": "Node2D"}),
        None,
    );
    assert!(!made.error, "{}", made.text);
    assert!(Path::new(&server.path("levels/level_one.tscn")).exists());
    let tree = tool(&server, "tree", json!({}), None);
    assert!(tree.text.starts_with("LevelOne (Node2D)"), "{}", tree.text);
    let again = tool(
        &server,
        "scene",
        json!({"action": "new", "path": "res://levels/level_one.tscn"}),
        None,
    );
    assert!(
        again.error,
        "never overwrite an existing scene: {}",
        again.text
    );
}

/// Godot runs a game on ONE thread, and the interpreter's object registries
/// (RandomNumberGenerator, Image, JSON, ...) are thread-local. Play booted the
/// game on the HTTP thread that served `/api/runtime/play` and stepped it on
/// another, so an RNG made in `_ready` was gone a frame later.
#[test]
fn objects_created_in_ready_survive_into_later_frames() {
    let _guard = serial();
    let server = boot(None);
    let written = tool(
        &server,
        "write_script",
        json!({
            "path": "res://rng.gd",
            "source": "extends Node2D\n\nvar rng := RandomNumberGenerator.new()\nvar draws := 0\n\nfunc _ready():\n\trng.set_seed(7)\n\nfunc _process(_delta):\n\tvar v := rng.randf_range(0.0, 1.0)\n\tif v >= 0.0 and v <= 1.0:\n\t\tdraws += 1\n",
            "attach_to": "."
        }),
        None,
    );
    assert!(!written.error, "{}", written.text);
    let report = tool(
        &server,
        "play",
        json!({"frames": 5, "probes": [{"node": ".", "props": ["draws"], "at": 5}]}),
        None,
    );
    assert!(report.text.contains("0 error(s)"), "{}", report.text);
    assert!(report.text.contains("draws=5"), "{}", report.text);
}

/// Play's scene is the tree's CURRENT scene, `res://` resolves against the
/// project at runtime, autoload scripts attach (initialising members from
/// constants), and a scene change frees the old scene and expands instances.
#[test]
fn play_boots_a_project_the_way_godot_does() {
    let _guard = serial();
    let server = boot(None);
    std::fs::write(
        server.path("project.godot"),
        format!("{PROJECT}\n[autoload]\nState=\"*res://state.gd\"\n"),
    )
    .unwrap();
    std::fs::write(
        server.path("state.gd"),
        "extends Node\nconst START := 3\nvar lives := START\n",
    )
    .unwrap();
    std::fs::write(
        server.path("sub.tscn"),
        "[gd_scene format=3]\n\n[node name=\"Sub\" type=\"Node2D\"]\n\n[node name=\"Inner\" type=\"Node2D\" parent=\".\"]\n",
    )
    .unwrap();
    std::fs::write(
        server.path("level.tscn"),
        "[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"PackedScene\" path=\"res://sub.tscn\" id=\"1_sub\"]\n\n[node name=\"Level\" type=\"Node2D\"]\n\n[node name=\"S\" parent=\".\" instance=ExtResource(\"1_sub\")]\n",
    )
    .unwrap();
    let root = server.dir.path().display().to_string();
    http(
        server.port(),
        "POST",
        "/api/project/open",
        &json!({"path": root}).to_string(),
        None,
        None,
    );
    let switched = tool(
        &server,
        "write_script",
        json!({
            "path": "res://switch.gd",
            "source": "extends Node2D\n\nfunc _ready():\n\tcall_deferred(\"_go\")\n\nfunc _go():\n\tprint(\"rc=\", get_tree().change_scene_to_file(\"res://level.tscn\"))\n",
            "attach_to": "."
        }),
        None,
    );
    assert!(!switched.error, "{}", switched.text);
    let report = tool(
        &server,
        "play",
        json!({"frames": 3, "keep_running": true, "probes": [{"node": "/root/State", "props": ["lives"], "at": 3}]}),
        None,
    );
    assert!(
        report.text.contains("rc=0") || report.text.contains("rc= 0"),
        "{}",
        report.text
    );
    assert!(
        report.text.contains("lives=3"),
        "the autoload's script attached: {}",
        report.text
    );
    let live = tool(
        &server,
        "tree",
        json!({"running": true, "node": "/root", "props": false}),
        None,
    )
    .text;
    let _ = tool(&server, "stop", json!({}), None);
    let order: Vec<&str> = live.lines().map(str::trim).collect();
    assert_eq!(
        order.get(1).copied(),
        Some("State (Node) [res://state.gd]"),
        "autoloads come first under /root: {live}"
    );
    assert!(live.contains("Level (Node2D)"), "{live}");
    assert!(
        !live.contains("Main (Node2D)"),
        "the old scene was freed: {live}"
    );
    assert!(
        live.contains("S (Node2D)") && live.contains("Inner (Node2D)"),
        "the instance expanded: {live}"
    );
}

/// A script that cannot be loaded at Play time must be reported, not leave its
/// node silently scriptless.
#[test]
fn a_script_that_fails_to_load_is_reported_in_the_play_report() {
    let _guard = serial();
    let server = boot(None);
    std::fs::write(server.path("broken.gd"), "extends Node\nfunc _ready(:\n").unwrap();
    let attached = tool(
        &server,
        "edit",
        json!({"ops": [{"op": "add", "type": "Node", "name": "B", "props": {"script": "res://broken.gd"}}], "show_tree": false}),
        None,
    );
    assert!(!attached.error, "{}", attached.text);
    let report = tool(&server, "play", json!({"frames": 1}), None);
    assert!(
        report.text.contains("f0 ERROR Script parse error") && report.text.contains("broken.gd"),
        "{}",
        report.text
    );
}

#[test]
fn mcp_refuses_requests_from_web_pages() {
    let _guard = serial();
    let server = boot(None);
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}).to_string();
    let send = |origin: &str| {
        let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: text/plain\r\nOrigin: {origin}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(req.as_bytes()).unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        raw.split_whitespace().nth(1).unwrap_or("").to_string()
    };
    assert_eq!(
        send("https://evil.example"),
        "403",
        "a page on another site must not drive the editor"
    );
    assert_eq!(send("http://localhost.evil.example"), "403");
    assert_eq!(
        send(&format!("http://localhost:{}", server.port())),
        "200",
        "the editor's own page may"
    );
    assert_eq!(send("http://[::1]:9"), "200");
    assert!(gdeditor::agent_mcp::origin_is_local(
        "http://127.0.0.1:8080"
    ));
    assert!(!gdeditor::agent_mcp::origin_is_local("null"));
}

#[test]
fn new_resource_then_undo_restores_the_previous_resource_whole() {
    let _guard = serial();
    let server = boot(None);
    let built = tool(
        &server,
        "edit",
        json!({"ops": [{"op": "add", "type": "CollisionShape2D", "name": "S", "props": {"shape": {"new": "RectangleShape2D", "size": "Vector2(32, 32)"}}}], "show_tree": false}),
        None,
    );
    assert!(!built.error, "{}", built.text);
    let id = {
        let st = server.handle.state().lock().unwrap();
        st.scene_tree
            .get_node_by_path("/root/Main/S")
            .unwrap()
            .raw()
    };
    let (status, _) = http(
        server.port(),
        "POST",
        "/api/property/resource/new",
        &json!({"node_id": id, "property": "shape", "class_name": "CircleShape2D"}).to_string(),
        None,
        None,
    );
    assert_eq!(status, 200);
    let read = |key: &str| {
        let st = server.handle.state().lock().unwrap();
        let nid = st.scene_tree.get_node_by_path("/root/Main/S").unwrap();
        st.scene_tree.get_node(nid).unwrap().get_property(key)
    };
    assert_eq!(
        read("__shape__class"),
        gdvariant::Variant::String("CircleShape2D".into())
    );
    assert_eq!(
        read("__shape__size"),
        gdvariant::Variant::Nil,
        "the old resource's keys do not leak into the new one"
    );
    let (status, _) = http(server.port(), "POST", "/api/undo", "{}", None, None);
    assert_eq!(status, 200);
    assert_eq!(
        read("__shape__class"),
        gdvariant::Variant::String("RectangleShape2D".into())
    );
    assert_eq!(
        read("__shape__size"),
        gdvariant::Variant::Vector2(gdcore::math::Vector2::new(32.0, 32.0)),
        "Undo brings the previous shape back with its size"
    );
}

#[test]
fn control_offsets_are_anchored_to_the_game_viewport_not_the_editor_panel() {
    let _guard = serial();
    let server = boot(None);
    // A full-rect Control: its anchors span the game's 1152x648 default
    // viewport, whatever size the editor panel happens to be.
    let built = tool(
        &server,
        "edit",
        json!({"ops": [
            {"op": "add", "type": "Control", "name": "Full", "props": {"anchor_right": 1.0, "anchor_bottom": 1.0}},
            {"op": "set", "node": "Full", "props": {"size": "Vector2(100, 50)"}}
        ], "show_tree": false}),
        None,
    );
    assert!(!built.error, "{}", built.text);
    let st = server.handle.state().lock().unwrap();
    let nid = st.scene_tree.get_node_by_path("/root/Main/Full").unwrap();
    let node = st.scene_tree.get_node(nid).unwrap();
    assert_eq!(
        node.get_property("offset_right"),
        gdvariant::Variant::Float(100.0 - 1152.0)
    );
    assert_eq!(
        node.get_property("offset_bottom"),
        gdvariant::Variant::Float(50.0 - 648.0)
    );
}

#[test]
fn a_key_held_when_the_game_quits_is_released() {
    let _guard = serial();
    let server = boot(None);
    let written = tool(
        &server,
        "write_script",
        json!({"path": "res://quit.gd", "source": "extends Node2D\n\nvar n := 0\n\nfunc _process(_d):\n\tn += 1\n\tif n == 3:\n\t\tget_tree().quit()\n", "attach_to": "."}),
        None,
    );
    assert!(!written.error, "{}", written.text);
    let report = tool(
        &server,
        "play",
        json!({"frames": 20, "inputs": [{"key": "A", "at": 1, "frames": 10}]}),
        None,
    );
    assert!(report.text.contains("quit"), "{}", report.text);
    let pressed = server.handle.state().lock().unwrap().pressed_keys.len();
    assert_eq!(pressed, 0, "the key is released however the run ends");
}

#[test]
fn the_errors_tab_stays_bounded_when_a_script_fails_every_frame() {
    let _guard = serial();
    let server = boot(None);
    let written = tool(
        &server,
        "write_script",
        json!({"path": "res://loud.gd", "source": "extends Node2D\n\nfunc _process(_d):\n\tself.missing_method()\n", "attach_to": "."}),
        None,
    );
    assert!(!written.error, "{}", written.text);
    let report = tool(&server, "play", json!({"frames": 1200}), None);
    assert!(
        report.text.contains("×1200"),
        "one collapsed line: {}",
        report.text
    );
    let n = server.handle.state().lock().unwrap().debug_errors.len();
    assert!((500..=1000).contains(&n), "{n} entries");
}

/// A player who takes a screenshot (Cmd+Ctrl+Shift+4) mid-game: the OS takes
/// the keyboard, the page never sees the modifier keyups, and the server keeps
/// Command held. "Meta" was an unknown key name, and one unknown held key
/// switched Play onto the editor's fallback snapshot — which has none of the
/// project's actions — so every `move_*` stopped working for good.
#[test]
fn a_stuck_command_key_does_not_freeze_the_game() {
    let _guard = serial();
    let server = boot(None);
    assert!(!build_player(&server).error);
    let report = tool(
        &server,
        "play",
        json!({
            "frames": 40,
            "inputs": [
                {"key": "Meta", "at": 1, "frames": 1000},
                {"key": "Control", "at": 1, "frames": 1000},
                {"key": "Shift", "at": 1, "frames": 1000},
                {"key": "D", "at": 5, "frames": 30}
            ],
            "probes": [{"node": "Player", "props": ["position"], "at": 35}]
        }),
        None,
    );
    assert!(!report.error, "{}", report.text);
    let probe = report
        .text
        .lines()
        .find(|l| l.contains("f35 Player:"))
        .unwrap_or_default()
        .to_string();
    let x: f64 = probe
        .split("Vector2(")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0.0);
    assert!(
        x > 150.0,
        "move_right must still fire with Command held: {probe}"
    );
}

/// The page releases every key it forwarded when it loses focus or Command is
/// let go, as Godot releases pressed input on focus-out.
#[test]
fn the_game_view_releases_held_keys_when_the_page_loses_focus() {
    let _guard = serial();
    let html = gdeditor::editor_ui::EDITOR_HTML;
    for needle in [
        "function releaseHeldGameKeys(",
        "window.addEventListener('blur', gameInputBlurHandler)",
        "document.addEventListener('visibilitychange', gameInputVisibilityHandler)",
        "if (e.key === 'Meta') releaseHeldGameKeys();",
    ] {
        assert!(
            html.contains(needle),
            "missing `{needle}` in the served editor"
        );
    }
}

/// The player's view is the frame the browser was last sent, not a fresh
/// render: the plain screenshot renders at the project's window size, which
/// looked right while the browser drew the game 1:1 in a panel's corner.
#[test]
fn the_player_view_is_the_frame_the_browser_was_sent() {
    let _guard = serial();
    let server = boot(None);
    let none = tool(&server, "screenshot", json!({"view": "player"}), None);
    assert!(none.error, "nothing published yet: {}", none.text);
    assert!(
        none.text.contains("no frame has been published"),
        "{}",
        none.text
    );

    let sent = gdrender2d::renderer::FrameBuffer::new(
        50,
        40,
        gdcore::math::Color::new(0.2, 0.4, 0.6, 1.0),
    );
    server.handle.update_frame(sent.clone());
    let shot = tool(&server, "screenshot", json!({"view": "player"}), None);
    assert!(!shot.error, "{}", shot.text);
    assert!(shot.text.contains("50x40"), "{}", shot.text);
    assert_eq!(
        shot.images,
        vec![gdrender2d::export::encode_png_fast(&sent)],
        "the image is exactly the published frame"
    );
}

/// `diagnose` reports what was measured: frames viewers received, requests
/// the rate limiter refused, and a held key that puts Play on the fallback
/// input snapshot (how a swallowed keyup froze a game's controls).
#[test]
fn diagnose_reports_served_frames_refused_requests_and_a_fallback_key() {
    let _guard = serial();
    let server = boot(None);
    let port = server.port();
    let (status, body) = http(port, "POST", "/api/runtime/play", "{}", None, None);
    assert_eq!(status, 200, "{body}");
    let (status, body) = http(
        port,
        "POST",
        "/api/runtime/input/key_down",
        r#"{"key":"§"}"#,
        None,
        None,
    );
    assert_eq!(status, 200, "{body}");
    // Four clients at once, so the burst crosses 60 requests in one second.
    let bursts: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(move || {
                (0..100)
                    .filter(|_| http(port, "GET", "/api/runtime/status", "", None, None).0 == 429)
                    .count()
            })
        })
        .collect();
    let refused: usize = bursts.into_iter().map(|b| b.join().unwrap()).sum();
    assert!(refused > 0, "the burst was never limited");
    // `/mcp` itself shares the caller's budget: let the 1 s window pass, or
    // the `diagnose` call below is refused too.
    std::thread::sleep(std::time::Duration::from_millis(1100));

    let started = std::time::Instant::now();
    server
        .handle
        .update_frame(gdrender2d::renderer::FrameBuffer::new(
            8,
            8,
            gdcore::math::Color::new(0.0, 0.0, 0.0, 1.0),
        ));
    for _ in 0..5 {
        assert_eq!(http(port, "GET", "/api/viewport", "", None, None).0, 200);
    }
    let report = tool(&server, "diagnose", json!({}), None);
    assert!(!report.error, "{}", report.text);
    let text = &report.text;
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    assert!(text.contains(&format!("build: {build}")), "{text}");
    assert!(text.contains("game: running"), "{text}");
    if started.elapsed() < std::time::Duration::from_millis(900) {
        assert!(text.contains("viewers: 5 frames/s served"), "{text}");
    }
    assert!(
        text.contains(&format!(
            "refused (last 10 s): /api/runtime/status ×{refused}"
        )),
        "{text}"
    );
    assert!(text.contains("held §"), "{text}");
    assert!(text.contains("input fallback: `§` is held"), "{text}");
    // Releasing the key ends the fallback.
    http(
        port,
        "POST",
        "/api/runtime/input/key_up",
        r#"{"key":"§"}"#,
        None,
        None,
    );
    let after = tool(&server, "diagnose", json!({}), None).text;
    assert!(!after.contains("input fallback"), "{after}");
    http(port, "POST", "/api/runtime/stop", "{}", None, None);
}

/// The running window's stretch is reported next to what project.godot asks
/// for, and named as a finding whenever the two disagree.
#[test]
fn diagnose_compares_the_running_windows_stretch_with_the_project() {
    let _guard = serial();
    let project = format!(
        "{PROJECT}\n[display]\nwindow/size/viewport_width=640\nwindow/size/viewport_height=360\nwindow/stretch/mode=\"canvas_items\"\nwindow/stretch/aspect=\"keep\"\n"
    );
    let server = boot_with_project(None, &project);
    let port = server.port();
    let idle = tool(&server, "diagnose", json!({}), None).text;
    assert!(
        idle.contains("window: (no game running) · project.godot: canvas_items/keep at 640x360"),
        "{idle}"
    );
    let (status, body) = http(port, "POST", "/api/runtime/play", "{}", None, None);
    assert_eq!(status, 200, "{body}");
    let text = tool(&server, "diagnose", json!({}), None).text;
    http(port, "POST", "/api/runtime/stop", "{}", None, None);
    let line = text
        .lines()
        .find(|l| l.starts_with("window: "))
        .unwrap_or_default();
    assert!(
        line.ends_with("· project.godot: canvas_items/keep at 640x360"),
        "{text}"
    );
    let running = line
        .split(", stretch ")
        .nth(1)
        .and_then(|rest| rest.split(" · ").next())
        .unwrap_or_default();
    assert_eq!(
        running != "canvas_items/keep at 640x360",
        text.contains("stretch ignored:") || text.contains("stretch differs:"),
        "a finding exactly when the running window disagrees with project.godot:\n{text}"
    );
}

/// A timeline `click` presses a Button. Found playtesting Dodge the Creeps
/// through this interface: the click was sent as DOM button 1 (MIDDLE — the
/// routes take browser numbering), so no Button ever fired and an agent could
/// not start the game.
#[test]
fn a_timeline_click_presses_a_button() {
    let _guard = serial();
    let server = boot(None);
    let built = tool(
        &server,
        "edit",
        json!({"ops": [
            {"op": "add", "type": "Button", "name": "Start",
             "props": {"position": "Vector2(20, 20)", "size": "Vector2(100, 40)", "text": "Start"}}
        ]}),
        None,
    );
    assert!(!built.error, "{}", built.text);
    let written = tool(
        &server,
        "write_script",
        json!({
            "path": "res://clicks.gd",
            "source": "extends Node2D\n\nvar pressed_count := 0\n\nfunc _ready():\n\t$Start.pressed.connect(_on_start)\n\nfunc _on_start():\n\tpressed_count += 1\n",
            "attach_to": "."
        }),
        None,
    );
    assert!(!written.error, "{}", written.text);
    let report = tool(
        &server,
        "play",
        json!({
            "frames": 10,
            "inputs": [{"click": [70, 40], "at": 3}],
            "probes": [{"node": ".", "props": ["pressed_count"], "at": 10}]
        }),
        None,
    );
    assert!(report.text.contains("0 error(s)"), "{}", report.text);
    assert!(
        report.text.contains("pressed_count=1"),
        "the click inside the Button presses it exactly once:\n{}",
        report.text
    );
}

/// Godot's key names work on a timeline (`Left`, not only the DOM's
/// `ArrowLeft`), and an unknown key name is an error rather than a silently
/// unpressed key. Found playtesting Dodge the Creeps: `"key": "Left"` held for
/// 40 frames reported a clean run in which the player never moved.
#[test]
fn godot_key_names_press_and_unknown_key_names_are_rejected() {
    let _guard = serial();
    let server = boot(None);
    let written = tool(
        &server,
        "write_script",
        json!({
            "path": "res://keys.gd",
            "source": "extends Node2D\n\nvar lefts := 0\n\nfunc _process(_delta):\n\tif Input.is_key_pressed(KEY_LEFT):\n\t\tlefts += 1\n",
            "attach_to": "."
        }),
        None,
    );
    assert!(!written.error, "{}", written.text);
    let report = tool(
        &server,
        "play",
        json!({
            "frames": 10,
            "inputs": [{"key": "Left", "at": 2, "frames": 5}],
            "probes": [{"node": ".", "props": ["lefts"], "at": 10}]
        }),
        None,
    );
    assert!(
        report.text.contains("lefts=5"),
        "`Left` holds the left arrow:\n{}",
        report.text
    );

    let bad = tool(
        &server,
        "play",
        json!({"frames": 3, "inputs": [{"key": "Lft", "at": 1}]}),
        None,
    );
    assert!(bad.error, "an unknown key must fail the call: {}", bad.text);
    assert!(bad.text.contains("unknown key `Lft`"), "{}", bad.text);
}

/// The audio tool and endpoint exist and say why they have nothing to measure
/// when no device is open (a headless test never opens one) — rather than
/// reporting a silent device as healthy.
#[test]
fn audio_without_a_device_is_reported_not_faked() {
    let _guard = serial();
    let server = boot(None);
    let (status, body) = http(
        server.port(),
        "GET",
        "/api/runtime/audio?secs=2",
        "",
        None,
        None,
    );
    assert_eq!(status, 200, "the route exists even with no device: {body}");
    assert!(
        body.contains("\"device\":null"),
        "no device is reported as null: {body}"
    );
    assert!(
        body.contains("audio: true"),
        "and says how to get one: {body}"
    );
    let r = tool(&server, "audio", json!({"secs": 2}), None);
    assert!(r.error, "the MCP tool surfaces the same error: {}", r.text);
    assert!(r.text.contains("no audio device"), "{}", r.text);
}
