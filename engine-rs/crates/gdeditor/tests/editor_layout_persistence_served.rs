//! pat-s3rrv.228 — the editor layout survives a server restart.
//!
//! Godot's `EditorDockManager` lets docks move between slots and resize, and
//! Editor > Editor Layout saves the arrangement to `editor_layout.cfg` so it
//! comes back next session.
//!
//! Patina's two menu items called `logMessage` and nothing else. Resizing DID
//! already work (three drag dividers), and `saveLayout()` existed — but it was
//! never called from anywhere, and the sizes it would have sent live in
//! `display_settings.panel_sizes`, which the server holds **in memory only**. So
//! an arrangement could not outlive the process.
//!
//! These drive the real endpoints and, crucially, read the layout back through a
//! SECOND server built from fresh state — which is the only way to show the
//! arrangement is really persisted rather than cached in the first server.

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

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
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
        "patina_layout_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Starts a server on `dir` with completely fresh `EditorState`.
///
/// Calling this twice for the same directory is the "restart" these tests turn
/// on: the second server shares nothing with the first except the project on
/// disk, so anything it can still see has genuinely been persisted.
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

/// The Inspector resized to 480 and FileSystem moved to the right column — the
/// exact arrangement the acceptance describes.
const SAVED_LAYOUT: &str = r#"{"layout":{"splits":{"left":240,"right":480,"bottom":320},"docks":{"left":["scene-panel"],"right":["inspector-panel","filesystem-panel"]}}}"#;

/// **The acceptance.** Save an arrangement, restart the server's state, and the
/// arrangement comes back.
#[test]
fn editor_layout_persistence_served() {
    let dir = unique_dir("persist");

    let first = serve(&dir);
    let saved = http_post(first, "/api/editor/layout", SAVED_LAYOUT);
    assert!(saved.contains("\"ok\":true"), "saving failed:\n{saved}");

    // A completely fresh server: new EditorState, nothing carried over but the
    // project directory. Reading the layout here is the whole point — the old
    // in-memory `panel_sizes` route could never have survived this.
    let second = serve(&dir);
    let resp = http_get(second, "/api/editor/layout");

    assert!(
        resp.contains("\"saved\":true"),
        "the restarted server must report a saved layout:\n{resp}"
    );
    assert!(
        resp.contains("\"right\":480"),
        "the resized Inspector width must survive the restart:\n{resp}"
    );
    assert!(
        resp.contains("\"bottom\":320"),
        "the bottom panel height must survive too:\n{resp}"
    );
    // The slot assignment, not just the sizes: FileSystem was moved right.
    assert!(
        resp.contains("filesystem-panel"),
        "the moved dock must be recorded in a slot:\n{resp}"
    );
    // Bounded to the right slot's own array: an unbounded `contains` would also
    // match filesystem-panel sitting in the LEFT list further along the payload,
    // and pass on exactly the bug this asserts against.
    let right_slot = resp
        .split("\"right\":[")
        .nth(1)
        .and_then(|t| t.split(']').next())
        .unwrap_or("");
    assert!(
        right_slot.contains("filesystem-panel"),
        "filesystem-panel must come back in the RIGHT slot, not the left one — \
         restoring sizes while dropping the arrangement is the bug this bead is \
         about (right slot was `{right_slot}`):\n{resp}"
    );

    // The served page must actually use the endpoint, rather than only logging.
    assert!(
        gdeditor::editor_ui::EDITOR_HTML.contains("/api/editor/layout"),
        "EDITOR_HTML must call /api/editor/layout"
    );
    // Positive form on purpose: a `!contains(<stub text>)` guard passes whenever
    // the needle is merely misspelled, so it would have gone green before the fix
    // too. Naming the function that must now be called cannot pass vacuously.
    assert!(
        gdeditor::editor_ui::EDITOR_HTML.contains("saveEditorLayout()"),
        "the Save Editor Layout menu item must call saveEditorLayout(), not just log"
    );
    assert!(
        gdeditor::editor_ui::EDITOR_HTML.contains("resetEditorLayout()"),
        "Reset to Default Layout must call resetEditorLayout(), not just log"
    );
}

/// Reset restores the shipped layout and forgets the saved one — across a
/// restart, so it is the file that was removed and not just a cached value.
#[test]
fn editor_layout_persistence_served_reset_restores_the_default() {
    let dir = unique_dir("reset");

    let first = serve(&dir);
    assert!(http_post(first, "/api/editor/layout", SAVED_LAYOUT).contains("\"ok\":true"));

    let reset = http_post(first, "/api/editor/layout", r#"{"reset":true}"#);
    assert!(reset.contains("\"ok\":true"), "reset failed:\n{reset}");
    assert!(
        reset.contains("\"right\":320"),
        "reset must answer with the shipped default width:\n{reset}"
    );

    let second = serve(&dir);
    let resp = http_get(second, "/api/editor/layout");
    assert!(
        resp.contains("\"saved\":false"),
        "after a reset the restarted server must report no saved layout:\n{resp}"
    );
    assert!(
        !resp.contains("\"right\":480"),
        "the discarded arrangement must not come back:\n{resp}"
    );
}

/// A project that never saved a layout gets the shipped default, not an error
/// and not an empty object the client would have to special-case.
#[test]
fn editor_layout_persistence_served_unsaved_returns_the_default() {
    let dir = unique_dir("fresh");
    let port = serve(&dir);

    let resp = http_get(port, "/api/editor/layout");
    assert!(
        resp.contains("\"saved\":false"),
        "a project with no saved layout reports saved:false:\n{resp}"
    );
    assert!(
        resp.contains("scene-panel") && resp.contains("inspector-panel"),
        "the default must name the shipped docks so the client can lay out \
         without hardcoding them:\n{resp}"
    );
}

/// Corrupt state degrades to the default instead of failing the editor's boot.
///
/// The layout file is plain JSON on disk that a user can edit or a crash can
/// truncate; if that bricked the layout endpoint the editor would come up wrong
/// with no obvious cause.
#[test]
fn editor_layout_persistence_served_corrupt_file_falls_back_to_default() {
    let dir = unique_dir("corrupt");
    let cfg = dir.join(".patina").join("editor");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("editor_layout.json"), "{not json").unwrap();

    let port = serve(&dir);
    let resp = http_get(port, "/api/editor/layout");
    assert!(
        resp.contains("\"saved\":false"),
        "an unparseable layout must read as 'nothing saved':\n{resp}"
    );
    assert!(
        resp.contains("\"right\":320"),
        "and must serve the shipped default:\n{resp}"
    );
}
