//! pat-0q2kx.241 — the `EditorPlugin` scripting surface reaches the real editor.
//!
//! Godot exposes `EditorCommandPalette`, `EditorContextMenuPlugin`,
//! `EditorDebuggerPlugin`, `EditorDebuggerSession` and `EditorDock` to `@tool`
//! scripts. Patina had none of them (`rg` over `engine-rs/crates` → 0 hits).
//!
//! The interesting half is not the ClassDB entries — it is that
//! `GET /api/commands` enumerated a `const EDITOR_COMMANDS` array, so a command
//! added by a script had **no path to the palette at all**. Registering the
//! classes without closing that gap would have produced five names that a
//! script could mention and nothing that a script could do.
//!
//! So these tests drive the **real HTTP server** and the **real plugin enable
//! path** (`plugin.cfg` on disk → `POST /api/plugins/toggle` → the script's
//! `_enter_tree`), then assert the command is visible on the endpoint and
//! executable through it.
//!
//! Binary named `editor_plugin_api`: the acceptance's own name,
//! `editor_plugin_scripting_api_registered`, would push the verify command to
//! 75 characters, past the 58-char truncation where the target silently
//! resolves to nothing.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdobject::class_db;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HTTP harness (mirrors editor_export_presets_e2e_test.rs)
// ---------------------------------------------------------------------------

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

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// The `@tool` plugin under test: it registers a palette command in
/// `_enter_tree`, exactly as a Godot addon does.
const PLUGIN_SCRIPT: &str = "\
@tool
extends EditorPlugin

var fired = 0

func _enter_tree():
    EditorCommandPalette.add_command(\"Say Hello\", \"demo/say_hello\", _on_say_hello, \"Ctrl+Alt+H\")

func _on_say_hello():
    fired += 1
    print(\"say_hello fired\")
";

/// A throwaway project containing one addon.
fn temp_project_with_plugin(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx241_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let addon = dir.join("addons").join("palette_demo");
    std::fs::create_dir_all(&addon).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"PluginApi\"\n",
    )
    .unwrap();
    std::fs::write(
        addon.join("plugin.cfg"),
        "[plugin]\nname=\"Palette Demo\"\nscript=\"palette_demo.gd\"\n",
    )
    .unwrap();
    std::fs::write(addon.join("palette_demo.gd"), PLUGIN_SCRIPT).unwrap();
    dir
}

/// Starts a server rooted at a temp project that ships the addon, and enables
/// the plugin — which is what runs `_enter_tree` and registers the command.
fn server_with_plugin_enabled(tag: &str) -> (EditorServerHandle, u16) {
    // The command store is process-global; nextest gives each test its own
    // process, but clearing keeps this honest if the runner ever changes.
    gdscript_interop::editor_commands::clear();

    let dir = temp_project_with_plugin(tag);
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    let open = http_post(
        port,
        "/api/project/open",
        &serde_json::json!({ "path": dir.display().to_string() }).to_string(),
    );
    assert!(
        open.contains("200 OK"),
        "/api/project/open must succeed, got: {open}"
    );

    // The addon must be discovered before it can be toggled.
    let plugins = json_of(&http_get(port, "/api/plugins"));
    let found = plugins["plugins"]
        .as_array()
        .map(|a| a.iter().any(|p| p["folder"] == "palette_demo"))
        .unwrap_or(false);
    assert!(
        found,
        "the addon must be discovered from addons/palette_demo, got: {plugins}"
    );

    let toggled = http_post(
        port,
        "/api/plugins/toggle",
        &serde_json::json!({ "name": "palette_demo", "enabled": true }).to_string(),
    );
    assert!(
        toggled.contains("200 OK"),
        "enabling the plugin must succeed, got: {toggled}"
    );

    (handle, port)
}

/// Pulls the command list off the endpoint.
fn commands(port: u16) -> Vec<serde_json::Value> {
    let resp = http_get(port, "/api/commands");
    assert!(resp.contains("200 OK"), "/api/commands failed: {resp}");
    json_of(&resp)["commands"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

// ===========================================================================
// The acceptance test
// ===========================================================================

/// **The acceptance test.** The five classes exist with Godot's parents, and a
/// `@tool` plugin's `EditorCommandPalette.add_command` reaches the served
/// editor's command endpoint.
///
/// The endpoint half is the part that proves wiring rather than declaration: a
/// ClassDB entry alone would satisfy the first block and leave the second one
/// failing.
#[test]
fn editor_plugin_scripting_api_registered() {
    // ClassDB is NOT populated for free in a test process — the registration
    // functions must be called explicitly, or every assertion below fails
    // whether or not the classes were ever written. `register_2d_classes`
    // brings `MarginContainer` and `RefCounted`, `register_root_window_chain`
    // brings `Window` (the base of the dialog chain), and the editor classes
    // are the ones under test.
    class_db::register_2d_classes();
    class_db::register_root_window_chain();
    class_db::register_editor_classes();
    gdobject::editor_plugin_classes::register_editor_plugin_classes();

    // -- the classes exist with Godot's parents ------------------------------
    for (class, parent) in [
        ("EditorCommandPalette", "ConfirmationDialog"),
        ("EditorContextMenuPlugin", "RefCounted"),
        ("EditorDebuggerPlugin", "RefCounted"),
        ("EditorDebuggerSession", "RefCounted"),
        ("EditorDock", "MarginContainer"),
    ] {
        assert!(
            class_db::class_exists(class),
            "{class} must be registered in ClassDB",
        );
        // `inheritance_chain` starts with the class itself, so [1] is the
        // immediate parent. Asserting the immediate parent — not just
        // `is_parent_class`, which is true for any ancestor — is what catches a
        // registration that attaches the class at the wrong depth.
        let chain = class_db::inheritance_chain(class);
        assert_eq!(
            chain.get(1).map(String::as_str),
            Some(parent),
            "{class} must extend {parent} directly as in Godot; chain was {chain:?}",
        );
        assert!(
            class_db::is_parent_class(class, parent),
            "the {class} -> {parent} chain must resolve",
        );
    }

    // -- a @tool plugin's command reaches the editor's endpoint --------------
    let (handle, port) = server_with_plugin_enabled("acceptance");

    let cmds = commands(port);
    let mine = cmds
        .iter()
        .find(|c| c["id"] == "demo/say_hello")
        .unwrap_or_else(|| {
            panic!(
                "the plugin's command must appear in GET /api/commands; got {} commands: {:?}",
                cmds.len(),
                cmds.iter().map(|c| &c["id"]).collect::<Vec<_>>(),
            )
        });
    assert_eq!(mine["label"], "Say Hello", "the palette label must survive");
    assert_eq!(
        mine["source"], "plugin",
        "a script command must be distinguishable from a built-in",
    );

    // Built-ins must still be present — the merge must add, not replace.
    assert!(
        cmds.iter().any(|c| c["source"].is_null()),
        "built-in commands must survive the merge",
    );

    handle.stop();
}

// ===========================================================================
// Wiring details
// ===========================================================================

/// The registered command is **invokable**, not just listed: executing it by id
/// runs the plugin's callback inside the owning script instance.
#[test]
fn a_plugin_command_executes_its_callback() {
    let (handle, port) = server_with_plugin_enabled("execute");

    let resp = http_post(
        port,
        "/api/command/execute",
        &serde_json::json!({ "command": "demo/say_hello" }).to_string(),
    );
    assert!(resp.contains("200 OK"), "execute must not 404: {resp}");

    let v = json_of(&resp);
    assert_eq!(v["ok"], true, "the callback must run: {v}");
    assert_eq!(v["action"], "plugin");
    assert_eq!(v["plugin"], "palette_demo", "the owning addon is reported");
    assert_eq!(v["callback"], "_on_say_hello");

    handle.stop();
}

/// The shortcut survives the trip, and Godot's literal `"None"` is not
/// advertised as though it were a real key binding.
#[test]
fn shortcut_text_reaches_the_endpoint() {
    let (handle, port) = server_with_plugin_enabled("shortcut");

    let cmds = commands(port);
    let mine = cmds.iter().find(|c| c["id"] == "demo/say_hello").unwrap();
    assert_eq!(mine["shortcut"], "Ctrl+Alt+H");

    gdscript_interop::editor_commands::add_command("N", "demo/none", None, Some("None".into()));
    let cmds = commands(port);
    let none_cmd = cmds.iter().find(|c| c["id"] == "demo/none").unwrap();
    assert!(
        none_cmd["shortcut"].is_null(),
        "Godot's \"None\" placeholder must not become a shortcut, got {none_cmd}",
    );

    handle.stop();
}

/// An unknown command still 404s — the plugin fallback must not turn the
/// endpoint into one that accepts anything.
#[test]
fn unknown_commands_still_fail() {
    let (handle, port) = server_with_plugin_enabled("unknown");

    let resp = http_post(
        port,
        "/api/command/execute",
        &serde_json::json!({ "command": "demo/does_not_exist" }).to_string(),
    );
    assert!(
        resp.contains("404"),
        "an unregistered command must still 404, got: {resp}"
    );

    handle.stop();
}

/// A plugin must not be able to shadow a built-in id: the palette would show
/// one command while `/api/command/execute` ran the built-in.
#[test]
fn a_plugin_cannot_shadow_a_builtin_command() {
    let (handle, port) = server_with_plugin_enabled("shadow");

    let before = commands(port);
    let builtin = before
        .iter()
        .find(|c| c["source"].is_null())
        .expect("there is at least one built-in command")
        .clone();
    let builtin_id = builtin["id"].as_str().unwrap().to_string();
    let builtin_label = builtin["label"].clone();

    gdscript_interop::editor_commands::add_command("Hijacked", &builtin_id, None, None);

    let after = commands(port);
    let matching: Vec<_> = after.iter().filter(|c| c["id"] == builtin_id).collect();
    assert_eq!(
        matching.len(),
        1,
        "the built-in id must appear exactly once, got {matching:?}",
    );
    assert_eq!(
        matching[0]["label"], builtin_label,
        "the built-in must win; a plugin cannot repoint an existing id",
    );

    handle.stop();
}
