//! pat-0q2kx.76: `@tool` scripts run in edit mode, and enabled addons persist.
//!
//! In Godot a script annotated `@tool` executes in the EDITOR as well as at
//! runtime, and enabling an addon instantiates the EditorPlugin named by its
//! `plugin.cfg`, with the enabled set written to `project.godot`
//! `[editor_plugins]` so it survives a restart.
//!
//! Patina had neither. `api_toggle_plugin` flipped a bool on `state.plugins` —
//! a list initialised `Vec::new()` that nothing ever populated from disk, so the
//! toggle could not even find a plugin to flip — and no `@tool` code ran in edit
//! mode at all. These drive the real HTTP server against a fixture project on
//! disk, because "survives a restart" cannot be observed in memory.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
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

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// JSON-escapes a path so it can be embedded in a request body (Windows
/// separators would otherwise read as escapes).
fn json_path(p: &Path) -> String {
    p.display().to_string().replace('\\', "\\\\")
}

/// A project with one addon and one `@tool`-scripted scene.
fn fixture_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("project.godot"),
        "config_version=5\n\n[application]\n\nconfig/name=\"ToolFixture\"\n",
    )
    .unwrap();

    let addon = root.join("addons").join("hello");
    std::fs::create_dir_all(&addon).unwrap();
    std::fs::write(
        addon.join("plugin.cfg"),
        "[plugin]\n\nname=\"Hello\"\ndescription=\"fixture\"\nversion=\"1.0\"\nscript=\"plugin.gd\"\n",
    )
    .unwrap();
    std::fs::write(
        addon.join("plugin.gd"),
        "@tool\nextends EditorPlugin\n\nfunc _enter_tree():\n    print(\"hello plugin entered\")\n",
    )
    .unwrap();

    // A @tool node script, and a plain one alongside it to prove the annotation
    // is what decides.
    std::fs::write(
        root.join("tool_node.gd"),
        "@tool\nextends Node2D\n\nfunc _ready():\n    print(\"tool node ready\")\n",
    )
    .unwrap();
    std::fs::write(
        root.join("plain_node.gd"),
        "extends Node2D\n\nfunc _ready():\n    print(\"plain node ready\")\n",
    )
    .unwrap();

    std::fs::write(
        root.join("main.tscn"),
        concat!(
            "[gd_scene load_steps=3 format=3]\n\n",
            "[ext_resource type=\"Script\" path=\"res://tool_node.gd\" id=\"1\"]\n",
            "[ext_resource type=\"Script\" path=\"res://plain_node.gd\" id=\"2\"]\n\n",
            "[node name=\"Main\" type=\"Node2D\"]\n\n",
            "[node name=\"ToolNode\" type=\"Node2D\" parent=\".\"]\n",
            "script = ExtResource(\"1\")\n\n",
            "[node name=\"PlainNode\" type=\"Node2D\" parent=\".\"]\n",
            "script = ExtResource(\"2\")\n",
        ),
    )
    .unwrap();

    dir
}

/// Boots a server and opens `root` as the project.
fn serve(root: &Path) -> (EditorServerHandle, u16) {
    gdobject::class_db::register_2d_classes();
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));
    let opened = json_of(&http_post(
        port,
        "/api/project/open",
        &format!(r#"{{"path":"{}"}}"#, json_path(root)),
    ));
    assert_eq!(opened["ok"], true, "project open failed: {opened}");
    (handle, port)
}

fn logs(port: u16) -> String {
    http_get(port, "/api/logs")
}

/// **The acceptance test.**
#[test]
fn editor_tool_plugins_e2e() {
    let dir = fixture_project();
    let root = dir.path();

    // -- the addon is discovered, and starts disabled -------------------------
    let (handle, port) = serve(root);
    let listed = json_of(&http_get(port, "/api/plugins"));
    let plugins = listed["plugins"].as_array().expect("plugins array");
    assert_eq!(
        plugins.len(),
        1,
        "addons/hello/plugin.cfg must be discovered: {listed}"
    );
    assert_eq!(plugins[0]["name"], "Hello", "name comes from plugin.cfg");
    assert_eq!(plugins[0]["enabled"], false, "it starts disabled");

    // -- enabling it runs _enter_tree and persists ----------------------------
    let toggled = json_of(&http_post(
        port,
        "/api/plugins/toggle",
        r#"{"name":"Hello","enabled":true}"#,
    ));
    assert_eq!(toggled["ok"], true, "toggle failed: {toggled}");
    assert_eq!(
        toggled["persisted"], true,
        "the enabled set must reach project.godot: {toggled}"
    );
    assert!(
        logs(port).contains("hello plugin entered"),
        "the plugin's _enter_tree print must reach the Output panel:\n{}",
        logs(port)
    );

    // The on-disk shape is Godot's, not ours.
    let project = std::fs::read_to_string(root.join("project.godot")).unwrap();
    assert!(
        project.contains("[editor_plugins]"),
        "project.godot must gain the section:\n{project}"
    );
    assert!(
        project.contains(r#"enabled=PackedStringArray("res://addons/hello/plugin.cfg")"#),
        "the enabled list must be Godot's PackedStringArray of cfg paths:\n{project}"
    );
    // The rest of the file must survive the rewrite.
    assert!(
        project.contains("config/name=\"ToolFixture\""),
        "rewriting project.godot must not drop existing settings:\n{project}"
    );
    handle.stop();

    // -- restart: the enabled state comes back from disk ----------------------
    //
    // ⚠️ This is the clause the old implementation could never satisfy: it only
    // ever flipped an in-memory bool.
    let (handle2, port2) = serve(root);
    let after = json_of(&http_get(port2, "/api/plugins"));
    assert_eq!(
        after["plugins"][0]["enabled"], true,
        "an enabled plugin must survive a server restart: {after}"
    );
    assert!(
        logs(port2).contains("hello plugin entered"),
        "…and must be started again on boot:\n{}",
        logs(port2)
    );

    // -- a @tool script in an opened scene runs _ready in edit mode -----------
    let loaded = json_of(&http_post(
        port2,
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, json_path(&root.join("main.tscn"))),
    ));
    assert_eq!(loaded["ok"], true, "scene load failed: {loaded}");

    let after_load = logs(port2);
    assert!(
        after_load.contains("tool node ready"),
        "a @tool script must run _ready in edit mode:\n{after_load}"
    );
    // ⚠️ The negative half is what proves the annotation is doing the work: a
    // script WITHOUT @tool must stay inert while editing, or every scene would
    // start running its game logic the moment it was opened.
    assert!(
        !after_load.contains("plain node ready"),
        "a script without @tool must NOT run in edit mode:\n{after_load}"
    );

    handle2.stop();
}

/// Disabling reverses both halves: the plugin stops and the file forgets it.
#[test]
fn disabling_a_plugin_stops_it_and_rewrites_project_godot() {
    let dir = fixture_project();
    let root = dir.path();
    std::fs::write(
        root.join("addons").join("hello").join("plugin.gd"),
        "@tool\nextends EditorPlugin\n\nfunc _enter_tree():\n    print(\"entered\")\n\nfunc _exit_tree():\n    print(\"exited\")\n",
    )
    .unwrap();

    let (handle, port) = serve(root);
    http_post(
        port,
        "/api/plugins/toggle",
        r#"{"name":"Hello","enabled":true}"#,
    );
    let off = json_of(&http_post(
        port,
        "/api/plugins/toggle",
        r#"{"name":"Hello","enabled":false}"#,
    ));
    assert_eq!(off["ok"], true, "disable failed: {off}");
    assert!(
        logs(port).contains("exited"),
        "_exit_tree must run on disable:\n{}",
        logs(port)
    );

    let project = std::fs::read_to_string(root.join("project.godot")).unwrap();
    assert!(
        project.contains("enabled=PackedStringArray()"),
        "disabling the last plugin leaves an empty array, as Godot does:\n{project}"
    );

    let listed = json_of(&http_get(port, "/api/plugins"));
    assert_eq!(listed["plugins"][0]["enabled"], false);
    handle.stop();
}

/// The toggle accepts the identities the UI and the project file each use.
#[test]
fn toggle_accepts_name_folder_or_cfg_path() {
    let dir = fixture_project();
    let (handle, port) = serve(dir.path());

    for ident in ["hello", "res://addons/hello/plugin.cfg", "Hello"] {
        let on = json_of(&http_post(
            port,
            "/api/plugins/toggle",
            &format!(r#"{{"name":"{ident}","enabled":true}}"#),
        ));
        assert_eq!(on["ok"], true, "toggle by {ident} failed: {on}");
    }

    let missing = http_post(
        port,
        "/api/plugins/toggle",
        r#"{"name":"nope","enabled":true}"#,
    );
    assert!(
        missing.contains("404"),
        "an unknown plugin is still a 404: {missing}"
    );
    handle.stop();
}

/// A `@tool` `_process` is ticked by the editor's idle frame, not just `_ready`.
///
/// ⚠️ `process_tool_scripts` with no caller would be the same defect this bead
/// exists to fix — code that exists, passes review, and never runs. This waits
/// on the real background driver rather than calling the function directly.
#[test]
fn tool_script_process_ticks_in_edit_mode() {
    let dir = fixture_project();
    let root = dir.path();
    std::fs::write(
        root.join("tool_node.gd"),
        r#"@tool
extends Node2D

var ticks = 0

func _ready():
    print("tool node ready")

func _process(delta):
    self.ticks += 1
    if self.ticks == 3:
        print("tool node ticked")
"#,
    )
    .unwrap();

    let (handle, port) = serve(root);
    let loaded = json_of(&http_post(
        port,
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, json_path(&root.join("main.tscn"))),
    ));
    assert_eq!(loaded["ok"], true, "scene load failed: {loaded}");

    // The driver ticks at the frame cadence; poll rather than sleeping a fixed
    // amount so a slow machine does not turn this into a flake.
    let mut seen = false;
    for _ in 0..40 {
        if logs(port).contains("tool node ticked") {
            seen = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        seen,
        "the editor idle frame must tick a @tool _process:\n{}",
        logs(port)
    );
    handle.stop();
}

/// pat-0q2kx.121 — a `@tool` script running in the editor sees
/// `Engine.is_editor_hint() == true`.
///
/// `set_engine_editor_hint` existed with zero real callers, so the flag was
/// stuck at its `false` initialiser and every `@tool` script took its *runtime*
/// branch while editing — the exact opposite of what the annotation means.
#[test]
fn tool_script_sees_editor_hint_in_edit_mode() {
    let dir = fixture_project();
    let root = dir.path();
    std::fs::write(
        root.join("tool_node.gd"),
        r#"@tool
extends Node2D

func _ready():
    if Engine.is_editor_hint():
        print("tool node sees editor hint")
    else:
        print("tool node sees runtime hint")
"#,
    )
    .unwrap();

    let (handle, port) = serve(root);
    let loaded = json_of(&http_post(
        port,
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, json_path(&root.join("main.tscn"))),
    ));
    assert_eq!(loaded["ok"], true, "scene load failed: {loaded}");

    let mut seen = false;
    for _ in 0..40 {
        if logs(port).contains("tool node sees editor hint") {
            seen = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let out = logs(port);
    assert!(
        seen,
        "a @tool script in the editor must see Engine.is_editor_hint() == true:\n{out}"
    );
    assert!(
        !out.contains("tool node sees runtime hint"),
        "the editor must not run a @tool script with the runtime hint:\n{out}"
    );
    handle.stop();
}
