//! pat-mgixk.43 — Project Settings is backed by `project.godot`.
//!
//! `GET`/`POST /api/project_settings` used to mutate 22 `EditorState` scalars
//! that nothing ever loaded from `project.godot` and nothing wrote back, so an
//! edit died with the server and Play never saw it. The richer
//! `project_settings_dialog.rs` was dead code AND keyed on Godot 3 names
//! (`physics/fps`, `display/resolution_w`) that a Godot 4 project does not read.
//!
//! These drive the real HTTP routes against a real `project.godot` on disk, the
//! way the browser dialog does: open a project, read its settings, write real
//! Godot 4.x keys, and reopen to prove they came from the file rather than from
//! memory.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// One request/response, returning the BODY only.
///
/// ⚠️ Returns the body, not the whole response: an earlier draft asserted
/// against the full text, where a header could satisfy a `contains` check and
/// make an assertion pass for the wrong reason.
fn http(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = if method == "GET" {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
    } else {
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    let text = String::from_utf8_lossy(&resp).to_string();
    match text.find("\r\n\r\n") {
        Some(i) => text[i + 4..].to_string(),
        None => text,
    }
}

fn json(port: u16, method: &str, path: &str, body: &str) -> serde_json::Value {
    let raw = http(port, method, path, body);
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{method} {path} not JSON ({e}):\n{raw}"))
}

fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_psettings_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project that already declares a setting, plus a **multi-line `[input]`
/// action**.
///
/// ⚠️ The input action is load-bearing, not scenery. Saving settings by
/// re-emitting `ProjectSettings::to_godot_format()` — the obvious
/// implementation, and the one the bead's text suggested — would destroy it:
/// `ProjectSettings::from_godot_format` reads the file a line at a time, so a
/// value spanning lines parses as the single token `{` and its events are lost,
/// and writing that back deletes the project's input map. Real fixtures in this
/// repo (`fixtures/conformance/input_action_move/project.godot`) are stored
/// exactly like this, so the hazard is live rather than hypothetical.
const PROJECT_GODOT: &str = r#"; Engine configuration file.
config_version=5

[application]

config/name="Fixture Game"
run/main_scene="res://main.tscn"

[input]

ui_accept={
"deadzone": 0.5,
"events": [null]
}
"#;

fn serve() -> u16 {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    // NOTE: `project_root` is deliberately NOT preset. The whole point is that
    // `POST /api/project/open` is what loads `project.godot`, so presetting it
    // would skip the code path under test.
    let state = EditorState::new(tree);
    let port = free_port();
    // `EditorServerHandle` has no `Drop`, so the listener thread outlives this
    // binding — same as `project_settings_autoload_e2e`, which is where this
    // harness comes from.
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    port
}

/// Finds a setting row by its real 4.x path in the `settings` table.
fn row<'a>(settings: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    settings
        .as_array()
        .unwrap_or_else(|| panic!("`settings` must be an array, got {settings}"))
        .iter()
        .flat_map(|sec| {
            sec.get("properties")
                .and_then(|p| p.as_array())
                .map(|v| v.as_slice())
                .unwrap_or(&[])
        })
        // ⚠️ No `.copied()`. `Iterator::find` returns `Option<Self::Item>`, and
        // the item here is already `&Value` — the extra `&` is only on the
        // PREDICATE's parameter. Adding `.copied()` asks for `Value: Copy`
        // (E0277) and yields `Value` where `&Value` is wanted (E0308).
        .find(|r| r.get("key").and_then(|k| k.as_str()) == Some(key))
        .unwrap_or_else(|| panic!("no `{key}` row in the settings table:\n{settings}"))
}

/// **The acceptance test (pat-mgixk.43).**
#[test]
fn editor_project_settings_persist_test() {
    let dir = unique_dir("persist");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    let port = serve();

    // -- open the project ----------------------------------------------------
    let open = json(
        port,
        "POST",
        "/api/project/open",
        &format!(
            r#"{{"path":"{}"}}"#,
            dir.to_string_lossy().replace('\\', "\\\\")
        ),
    );
    assert_eq!(
        open.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "opening the fixture project must succeed: {open}"
    );

    // -- 1. a key the project already declares shows on the FIRST get --------
    let first = json(port, "GET", "/api/project_settings", "");
    let settings = first
        .get("settings")
        .unwrap_or_else(|| panic!("GET must expose a real-key `settings` table:\n{first}"));
    let name = row(settings, "application/config/name");
    assert_eq!(
        name.get("value").and_then(|v| v.as_str()),
        Some("Fixture Game"),
        "a setting already in project.godot must be reported from the FILE, not \
         from the editor's hardcoded default: {name}"
    );
    assert_eq!(
        name.get("overridden").and_then(|v| v.as_bool()),
        Some(true),
        "a key the project declares is overridden, which is what drives Godot's \
         revert arrow: {name}"
    );
    // The engine default must travel with the row, or the dialog has nothing to
    // revert TO.
    assert!(
        name.get("default").is_some(),
        "each row carries its engine default for revert: {name}"
    );

    // -- 2. POST real Godot 4.x keys ----------------------------------------
    let posted = json(
        port,
        "POST",
        "/api/project_settings",
        r#"{"physics/common/physics_ticks_per_second":30,
            "display/window/stretch/mode":"canvas_items"}"#,
    );
    assert_eq!(
        posted.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "the POST must succeed: {posted}"
    );
    assert_eq!(
        posted.get("saved").and_then(|v| v.as_u64()),
        Some(2),
        "both keys must have reached project.godot; `saved` counts the writes \
         that landed, so 0 here means the route still only touched memory: \
         {posted}"
    );

    // -- 3. they are on disk, under the right INI sections -------------------
    let written = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    assert!(
        written.contains("[physics]"),
        "project.godot must gain a [physics] section:\n{written}"
    );
    assert!(
        written.contains("common/physics_ticks_per_second=30"),
        "the real 4.x key must be written under [physics] with the section \
         prefix stripped, Godot's own spelling:\n{written}"
    );
    assert!(
        written.contains("[display]"),
        "project.godot must gain a [display] section:\n{written}"
    );
    assert!(
        written.contains(r#"window/stretch/mode="canvas_items""#),
        "a string setting must be written QUOTED, or re-parsing yields a \
         different type:\n{written}"
    );
    // Godot 3 spellings must not appear: that was the dead dialog's bug.
    assert!(
        !written.contains("physics/fps") && !written.contains("display/stretch_mode"),
        "no Godot 3 key names may be written:\n{written}"
    );

    // -- 4. nothing else in the file was destroyed ---------------------------
    //
    // This is the assertion that would catch a `to_godot_format()` save.
    assert!(
        written.contains(r#"config/name="Fixture Game""#),
        "the pre-existing application/config/name must survive the save:\n{written}"
    );
    assert!(
        written.contains("[input]") && written.contains("ui_accept="),
        "the project's [input] map must survive a settings save:\n{written}"
    );
    assert!(
        written.contains("\"deadzone\": 0.5") && written.contains("\"events\""),
        "the multi-line input action must survive INTACT — losing its body is \
         exactly what re-emitting the parsed settings table would do:\n{written}"
    );

    // -- 5. reopening reads them back from the file --------------------------
    //
    // Re-open rather than just GET again: a second GET could be answered from
    // the in-memory table and would pass even if nothing had been persisted.
    let reopen = json(
        port,
        "POST",
        "/api/project/open",
        &format!(
            r#"{{"path":"{}"}}"#,
            dir.to_string_lossy().replace('\\', "\\\\")
        ),
    );
    assert_eq!(
        reopen.get("ok").and_then(|v| v.as_bool()),
        Some(true),
        "reopening must succeed: {reopen}"
    );

    let second = json(port, "GET", "/api/project_settings", "");
    let settings2 = second.get("settings").expect("settings table on reopen");
    assert_eq!(
        row(settings2, "physics/common/physics_ticks_per_second")
            .get("value")
            .and_then(|v| v.as_i64()),
        Some(30),
        "the physics tick rate must come back from project.godot after a reopen:\n{second}"
    );
    assert_eq!(
        row(settings2, "display/window/stretch/mode")
            .get("value")
            .and_then(|v| v.as_str()),
        Some("canvas_items"),
        "the stretch mode must come back from project.godot after a reopen:\n{second}"
    );

    // -- 6. and the dialog's own fields reflect the file ---------------------
    //
    // `categories` is the shape the browser dialog renders. Before this bead its
    // values were editor-local defaults, so a project with 30 ticks still showed
    // 60; opening a project now seeds them from the real settings.
    let physics_fps = second
        .get("categories")
        .and_then(|c| c.as_array())
        .and_then(|cats| {
            cats.iter()
                .find(|c| c.get("id").and_then(|i| i.as_str()) == Some("physics"))
        })
        .and_then(|c| c.get("properties"))
        .and_then(|p| p.as_array())
        .and_then(|props| {
            props
                .iter()
                .find(|p| p.get("key").and_then(|k| k.as_str()) == Some("physics_fps"))
        })
        .and_then(|p| p.get("value"))
        .and_then(|v| v.as_i64());
    assert_eq!(
        physics_fps,
        Some(30),
        "the dialog's Physics FPS field must show the project's value, not the \
         editor's hardcoded 60:\n{second}"
    );
}
