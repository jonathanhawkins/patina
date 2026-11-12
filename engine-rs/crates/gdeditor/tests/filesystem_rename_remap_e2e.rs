//! pat-0q2kx.70 — renaming a file rewrites every scene that references it.
//!
//! Godot's `FileSystemDock::_try_move_item` updates all owners as part of a
//! move: every `.tscn`/`.tres` whose `ext_resource` points at the moved file is
//! rewritten, so scenes never break. Patina's `/api/filesystem/rename` was a
//! bare `std::fs::rename` returning `{"ok":true}` — the file moved and every
//! scene pointing at it silently lost its texture.
//!
//! The reverse-dependency scan already existed (the file-info endpoint uses it
//! for `owners`); it simply had no caller here.
//!
//! These drive the real HTTP endpoints, so a pass means the dock's context menu
//! is wired end to end, not that a helper exists.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// The rename endpoint sandboxes against the process CWD, so these tests move
/// the CWD to their fixture. `cargo nextest` gives each test its own process;
/// this lock only matters under `cargo test`, where one process runs them all.
static CWD_LOCK: Mutex<()> = Mutex::new(());

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

/// Process-unique fixture dir — a fixed `/tmp` path collides across the
/// concurrent test processes in the verifier lane.
fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_fs_remap_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A scene referencing `res://a.png` through an `ext_resource`, as Godot writes it.
const SCENE_TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://a.png" id="1_tex"]

[node name="Root" type="Node2D"]

[node name="Sprite" type="Sprite2D" parent="."]
texture = ExtResource("1_tex")
"#;

/// Starts a server whose CWD (and therefore sandbox root) is `dir`.
fn serve(dir: &std::path::Path) -> u16 {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();

    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());

    let port = free_port();
    // `EditorServerHandle` has no `Drop`, so the listener thread outlives this
    // helper and serves for the rest of the test.
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    port
}

/// **The acceptance.** Rename the texture and the scene that used it follows.
#[test]
fn filesystem_rename_remap_e2e() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("accept");
    std::fs::write(dir.join("a.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    std::fs::write(dir.join("scene.tscn"), SCENE_TSCN).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let port = serve(&dir);

    let resp = http_post(
        port,
        "/api/filesystem/rename",
        r#"{"old_path":"res://a.png","new_name":"b.png"}"#,
    );
    assert!(resp.contains("\"ok\":true"), "rename failed:\n{resp}");
    assert!(
        resp.contains("scene.tscn"),
        "the response must report the scenes it rewrote, so the client can toast \
         them — got:\n{resp}"
    );

    // The file moved...
    assert!(dir.join("b.png").is_file(), "the renamed file exists");
    assert!(!dir.join("a.png").exists(), "the old name is gone");

    // ...and the dependent scene points at the new name.
    let scene = std::fs::read_to_string(dir.join("scene.tscn")).unwrap();
    assert!(
        scene.contains(r#"path="res://b.png""#),
        "the dependent scene must reference the new path:\n{scene}"
    );
    assert!(
        !scene.contains("res://a.png"),
        "no reference to the old path may survive:\n{scene}"
    );
    // Formatting is preserved — only the quoted path changed.
    assert!(
        scene.contains(r#"[ext_resource type="Texture2D" path="res://b.png" id="1_tex"]"#),
        "the ext_resource line must keep its other attributes:\n{scene}"
    );

    // The scene still loads, which is what the rewrite exists to guarantee.
    // `/api/scene/load` takes a filesystem path, not a `res://` one — it reads
    // the string straight through `std::fs::read_to_string` — and the CWD is
    // this fixture, so the bare name resolves.
    let resp = http_post(port, "/api/scene/load", r#"{"path":"scene.tscn"}"#);
    assert!(
        resp.contains("\"ok\":true") || resp.contains("200 OK"),
        "the rewritten scene must still load:\n{resp}"
    );
}

/// A rename nothing depends on reports an empty list rather than failing, and
/// leaves unrelated scenes untouched.
#[test]
fn rename_without_dependents_touches_nothing() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("nodeps");
    std::fs::write(dir.join("lonely.png"), b"x").unwrap();
    std::fs::write(dir.join("scene.tscn"), SCENE_TSCN).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let port = serve(&dir);
    let resp = http_post(
        port,
        "/api/filesystem/rename",
        r#"{"old_path":"res://lonely.png","new_name":"still_lonely.png"}"#,
    );
    assert!(resp.contains("\"ok\":true"), "rename failed:\n{resp}");
    assert!(
        resp.contains("\"updated_files\":[]"),
        "nothing referenced it, so nothing was rewritten:\n{resp}"
    );

    let scene = std::fs::read_to_string(dir.join("scene.tscn")).unwrap();
    assert!(
        scene.contains("res://a.png"),
        "an unrelated scene must be left exactly as it was:\n{scene}"
    );
}

/// The rewrite matches the whole quoted path: a file whose name is a prefix of
/// another's must not drag its neighbour along.
#[test]
fn rename_does_not_rewrite_longer_paths_that_share_a_prefix() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("prefix");
    std::fs::write(dir.join("a.png"), b"x").unwrap();
    std::fs::write(dir.join("a.png.import"), b"x").unwrap();
    let scene = "[gd_scene load_steps=3 format=3]\n\n\
         [ext_resource type=\"Texture2D\" path=\"res://a.png\" id=\"1\"]\n\
         [ext_resource type=\"Resource\" path=\"res://a.png.import\" id=\"2\"]\n\n\
         [node name=\"Root\" type=\"Node2D\"]\n";
    std::fs::write(dir.join("scene.tscn"), scene).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let port = serve(&dir);
    let resp = http_post(
        port,
        "/api/filesystem/rename",
        r#"{"old_path":"res://a.png","new_name":"b.png"}"#,
    );
    assert!(resp.contains("\"ok\":true"), "rename failed:\n{resp}");

    let scene = std::fs::read_to_string(dir.join("scene.tscn")).unwrap();
    assert!(
        scene.contains(r#"path="res://b.png""#),
        "the exact path was rewritten:\n{scene}"
    );
    assert!(
        scene.contains(r#"path="res://a.png.import""#),
        "a longer path sharing the prefix must be left alone — substring \
         replacement would have corrupted it:\n{scene}"
    );
}
