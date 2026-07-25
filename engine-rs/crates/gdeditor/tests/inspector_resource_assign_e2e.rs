//! pat-0q2kx.13 — the inspector's resource picker actually assigns resources.
//!
//! Godot's `EditorResourcePicker` assigns via New <Type>, Quick Load / Load, and a
//! drag from the FileSystem dock (upstream/godot/editor/inspector/editor_resource_picker.cpp).
//! Patina's picker had the menu but not the behaviour: `New`/`Load...` fell through
//! to `logMessage` and `.res-picker` rows accepted no drops, so a texture/material/
//! shape property could only ever be cleared.
//!
//! Both halves are guarded here, because either alone is a lie:
//!
//!  1. **Static** — the served HTML calls the assign/new endpoints, wires
//!     dragover/drop onto the picker row, and no longer stubs New/Load out to the
//!     log.
//!  2. **Functional** — assigning a file and creating a typed inline resource go
//!     through the real HTTP handlers and land in the saved `.tscn` in Godot's
//!     form: an `[ext_resource]` header with `texture = ExtResource(...)`, and a
//!     `[sub_resource]` section with `shape = SubResource(...)`.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Harness (same shape as the sibling editor wired-tests)
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

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

/// Process-unique fixture dir — fixed `/tmp` paths collide across concurrent test
/// processes in the verifier lane.
fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_res_picker_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---------------------------------------------------------------------------
// (1) Static wiring guard
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_resource_picker_actions() {
    for needle in [
        "/api/property/resource/assign",
        "/api/property/resource/new",
        "RESOURCE_SUBCLASSES",
        "collectAssignableFiles",
        "pathSuitsResType",
    ] {
        assert!(
            EDITOR_HTML.contains(needle),
            "served HTML is missing resource-picker wiring: {needle}"
        );
    }

    // Narrow to the picker itself, so a drop handler elsewhere in the file cannot
    // make this pass.
    let picker = EDITOR_HTML
        .split("function makeResourcePicker")
        .nth(1)
        .expect("makeResourcePicker exists")
        .split("function closeResMenu")
        .next()
        .expect("picker body is delimited");

    for needle in [
        "dragover",
        "drop",
        "insp-drop-active",
        "assignResource(",
        "openNewMenu",
        "openLoadMenu",
    ] {
        assert!(
            picker.contains(needle),
            "makeResourcePicker is missing {needle} — the row cannot accept a FileSystem drag"
        );
    }

    // The stub this bead exists to remove: New/Load used to log and do nothing.
    assert!(
        !picker.contains("logMessage('info', it[0]"),
        "New/Load are still logMessage stubs:\n{picker}"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional: assignment reaches the saved scene in Godot's form
// ---------------------------------------------------------------------------

/// The bead's acceptance: assign `res://icon.png` to a `Sprite2D.texture` through
/// the endpoint the picker uses, create a `New RectangleShape2D` on a
/// `CollisionShape2D`, then save — the `.tscn` must carry an `ext_resource` for
/// the texture and a `sub_resource` for the shape.
#[test]
fn inspector_resource_assign_e2e() {
    let dir = unique_dir("assign");
    // A real file on disk so the assignment refers to something that exists.
    std::fs::write(dir.join("icon.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    let scene_path = dir.join("main.tscn");

    // Scene: Root -> { Sprite (Sprite2D), Body (CollisionShape2D) }, both with
    // empty resource properties, so a pass cannot come from pre-existing values.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let sprite = tree
        .add_child(scene_root, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    let body = tree
        .add_child(scene_root, Node::new("Body", "CollisionShape2D"))
        .unwrap();
    let (sprite_raw, body_raw) = (sprite.raw(), body.raw());

    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // -- Load / Quick Load / FileSystem drop all land here --------------------
    let resp = http_post(
        port,
        "/api/property/resource/assign",
        &format!(r#"{{"node_id":{sprite_raw},"property":"texture","path":"res://icon.png"}}"#),
    );
    assert!(
        resp.contains("\"ok\":true"),
        "assigning a texture failed:\n{resp}"
    );

    // -- New <Type> ----------------------------------------------------------
    let resp = http_post(
        port,
        "/api/property/resource/new",
        &format!(r#"{{"node_id":{body_raw},"property":"shape","class_name":"RectangleShape2D"}}"#),
    );
    assert!(
        resp.contains("\"ok\":true"),
        "creating a New RectangleShape2D failed:\n{resp}"
    );

    // A name that is not a class must be refused rather than writing a
    // `[sub_resource type="…"]` Godot cannot load.
    let resp = http_post(
        port,
        "/api/property/resource/new",
        &format!(r#"{{"node_id":{body_raw},"property":"shape","class_name":"not a class"}}"#),
    );
    assert!(
        resp.contains("400"),
        "a malformed class name must be rejected:\n{resp}"
    );

    // -- Save, then assert Godot's on-disk form ------------------------------
    let resp = http_post(
        port,
        "/api/scene/save",
        &format!(
            r#"{{"path":"{}"}}"#,
            scene_path.to_string_lossy().replace('\\', "\\\\")
        ),
    );
    assert!(!resp.is_empty(), "save returned nothing");

    let saved = std::fs::read_to_string(&scene_path)
        .unwrap_or_else(|e| panic!("saved scene unreadable ({e}); save response was:\n{resp}"));

    assert!(
        saved.contains(r#"[ext_resource type="Texture2D" path="res://icon.png""#),
        "assigned texture did not become an ext_resource:\n{saved}"
    );
    assert!(
        saved.contains("texture = ExtResource("),
        "texture property is not an ExtResource reference:\n{saved}"
    );
    assert!(
        saved.contains(r#"[sub_resource type="RectangleShape2D""#),
        "New RectangleShape2D did not become a sub_resource:\n{saved}"
    );
    assert!(
        saved.contains("shape = SubResource("),
        "shape property is not a SubResource reference:\n{saved}"
    );
    // Bookkeeping companions are internal and must never reach the file.
    assert!(
        !saved.contains("__extres__") && !saved.contains("__resref__"),
        "internal companions leaked into the saved scene:\n{saved}"
    );

    drop(handle);
}
