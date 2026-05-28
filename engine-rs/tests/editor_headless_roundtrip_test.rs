//! pat-vnt3j: Acceptance gate for the headless curl-driven roundtrip script.
//!
//! Acceptance: `scripts/editor_headless_roundtrip.sh` uses only curl (and a
//! small amount of `jq` for JSON extraction) to create three nodes, save the
//! scene to a `.tscn` file on disk, reload it from disk, and assert the tree
//! matches. The script must exit 0 on success. This test boots a real
//! `editor_server`, runs the script against it, and fails if the script
//! exits non-zero — proving an agent armed with nothing but a shell and an
//! HTTP client can drive the editor end to end.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use tempfile::tempdir;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_editor_with_main_scene() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);

    // Wait until the listener is actually answering before handing the port
    // off to the curl-driven script.
    for _ in 0..40 {
        if TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(50),
        )
        .is_ok()
        {
            let _ = poke_health(port);
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    (handle, port)
}

fn poke_health(port: u16) -> std::io::Result<()> {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}"))?;
    s.set_read_timeout(Some(Duration::from_millis(500)))?;
    s.write_all(b"GET /api/scene HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    Ok(())
}

fn repo_root() -> PathBuf {
    // engine-rs/tests/<file>.rs → repo root is two parents up from CARGO_MANIFEST_DIR.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("engine-rs has a parent")
        .to_path_buf()
}

fn binary_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn editor_headless_roundtrip_script_drives_editor_end_to_end() {
    if !binary_available("curl") || !binary_available("jq") {
        eprintln!(
            "skipping editor_headless_roundtrip_test: requires `curl` and `jq` in PATH; \
             this environment is missing one of them"
        );
        return;
    }

    let script = repo_root().join("scripts").join("editor_headless_roundtrip.sh");
    assert!(
        script.exists(),
        "headless roundtrip script missing at {}",
        script.display()
    );

    let dir = tempdir().expect("tempdir for scene save");
    let scene_path = dir.path().join("roundtrip_scene.tscn");

    let (handle, port) = start_editor_with_main_scene();

    let output = Command::new("bash")
        .arg(&script)
        .arg(port.to_string())
        .arg(&scene_path)
        .output()
        .expect("spawn editor_headless_roundtrip.sh");

    handle.stop();

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "editor_headless_roundtrip.sh exited {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            output.status.code(),
            stdout,
            stderr
        );
    }

    // Script claims success only after re-reading the on-disk file. Sanity
    // check the file is still there and looks like a Patina/Godot scene.
    let scene_text = std::fs::read_to_string(&scene_path).expect("read roundtripped scene file");
    assert!(
        scene_text.contains("[gd_scene") || scene_text.contains("[node "),
        "roundtripped scene file does not look like a .tscn: {scene_text:?}"
    );
}
