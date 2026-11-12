//! pat-0q2kx.73 — Project Settings ▸ Autoload: CRUD persisted to project.godot.
//!
//! The runtime half already worked (`instantiate_play_autoloads` reads the
//! `[autoload]` section on play) and `autoload_manager.rs` already held the list
//! logic — but nothing connected them. The module was orphaned: `pub mod
//! autoload_manager;` was its only reference in the whole crate, there was no
//! `/api/autoload*` route, and the settings dialog had no such tab. The section
//! could only be edited by hand.
//!
//! These drive the real HTTP routes against a real `project.godot` on disk.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// The routes resolve `project.godot` under the project root, which falls back
/// to the process CWD. `cargo nextest` isolates each test in its own process;
/// this lock only matters under `cargo test`.
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
    String::from_utf8_lossy(&resp).to_string()
}

fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_autoload_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project with other sections present, so the splice has something to preserve.
const PROJECT_GODOT: &str = "[application]\n\n\
     config/name=\"Demo\"\n\
     run/main_scene=\"res://main.tscn\"\n\n\
     [input]\n\n\
     ui_accept={\"deadzone\":0.5}\n";

fn serve(dir: &std::path::Path) -> u16 {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());
    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    port
}

/// **The acceptance.** Add writes the `[autoload]` key, GET lists it, remove
/// deletes it, and the dialog carries the tab.
#[test]
fn project_settings_autoload_e2e() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("crud");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    std::fs::write(dir.join("game.gd"), "extends Node\n").unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let port = serve(&dir);

    // -- the dialog has the tab ----------------------------------------------
    assert!(
        EDITOR_HTML.contains(r#"data-cat="autoload""#),
        "the project-settings dialog must carry an Autoload category"
    );
    assert!(
        EDITOR_HTML.contains("pset-autoloads") && EDITOR_HTML.contains("renderAutoloads"),
        "the Autoload category must be wired to the route, not just listed"
    );

    // -- add ------------------------------------------------------------------
    let resp = http(
        port,
        "POST",
        "/api/autoloads",
        r#"{"action":"add","name":"Game","path":"res://game.gd"}"#,
    );
    assert!(resp.contains("\"ok\":true"), "add failed:\n{resp}");

    // Godot's on-disk form: enabled autoloads carry the `*` prefix.
    let written = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    assert!(
        written.contains(r#"Game="*res://game.gd""#),
        "project.godot must carry the [autoload] key in Godot's form:\n{written}"
    );
    assert!(
        written.contains("[autoload]"),
        "the section header must exist:\n{written}"
    );
    // The splice must not eat the rest of the file.
    assert!(
        written.contains(r#"config/name="Demo""#) && written.contains("ui_accept="),
        "other sections must survive the write:\n{written}"
    );

    // -- list -----------------------------------------------------------------
    let resp = http(port, "GET", "/api/autoloads", "");
    assert!(
        resp.contains("\"Game\"") && resp.contains("res://game.gd"),
        "GET must list the autoload:\n{resp}"
    );
    assert!(
        resp.contains("\"enabled\":true"),
        "a new autoload is enabled:\n{resp}"
    );

    // -- remove ---------------------------------------------------------------
    let resp = http(
        port,
        "POST",
        "/api/autoloads",
        r#"{"action":"remove","name":"Game"}"#,
    );
    assert!(resp.contains("\"ok\":true"), "remove failed:\n{resp}");

    let written = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    assert!(
        !written.contains("Game="),
        "removal must delete the key:\n{written}"
    );
    assert!(
        written.contains(r#"config/name="Demo""#),
        "removal must still preserve other sections:\n{written}"
    );
}

/// Toggling writes the enabled flag through as Godot's `*` prefix, and reorder
/// changes the load order on disk.
#[test]
fn autoload_toggle_and_reorder_persist() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("order");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    for (name, path) in [("First", "res://a.gd"), ("Second", "res://b.gd")] {
        let resp = http(
            port,
            "POST",
            "/api/autoloads",
            &format!(r#"{{"action":"add","name":"{name}","path":"{path}"}}"#),
        );
        assert!(resp.contains("\"ok\":true"), "add {name} failed:\n{resp}");
    }

    // Disabling drops the `*`, which is exactly what Godot's Enable column means.
    let resp = http(
        port,
        "POST",
        "/api/autoloads",
        r#"{"action":"toggle","name":"First","enabled":false}"#,
    );
    assert!(resp.contains("\"ok\":true"), "toggle failed:\n{resp}");
    let written = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    assert!(
        written.contains(r#"First="res://a.gd""#),
        "a disabled autoload loses the star:\n{written}"
    );
    assert!(
        written.contains(r#"Second="*res://b.gd""#),
        "the other entry keeps its star:\n{written}"
    );

    // Reorder: Second moves ahead of First, and the file records the new order.
    let resp = http(
        port,
        "POST",
        "/api/autoloads",
        r#"{"action":"move_up","name":"Second"}"#,
    );
    assert!(resp.contains("\"ok\":true"), "move_up failed:\n{resp}");
    let written = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    let second_at = written.find("Second=").expect("Second present");
    let first_at = written.find("First=").expect("First present");
    assert!(
        second_at < first_at,
        "load order is the file order, and reorder must persist it:\n{written}"
    );
}

/// A duplicate name and an unknown name are refused rather than silently
/// no-oping, so the dialog can explain why nothing changed.
#[test]
fn duplicate_and_missing_names_are_refused() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("errors");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let add = r#"{"action":"add","name":"Game","path":"res://game.gd"}"#;
    assert!(http(port, "POST", "/api/autoloads", add).contains("\"ok\":true"));

    let resp = http(port, "POST", "/api/autoloads", add);
    assert!(
        resp.contains("409"),
        "adding the same name twice must be refused:\n{resp}"
    );

    let resp = http(
        port,
        "POST",
        "/api/autoloads",
        r#"{"action":"remove","name":"NeverExisted"}"#,
    );
    assert!(
        resp.contains("409"),
        "removing an unknown autoload must be refused:\n{resp}"
    );
}
