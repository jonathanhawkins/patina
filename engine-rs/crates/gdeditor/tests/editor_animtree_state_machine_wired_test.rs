//! pat-nzgdu.103: the served AnimationTree state-machine editor is backed by
//! real server routes and real graph state.
//!
//! Before this bead the AnimationTree view was a static shell: `editor_ui.rs`
//! hardcoded four `<div class="anim-tree-node">` entries ("idle", "walk",
//! "run") that no server ever produced, there were no `/api/animation/tree/*`
//! routes at all, and `anim_tree_graph.rs` — a complete graph model with
//! add_node/add_transition/has_transition — had zero callers outside its own
//! `pub mod` line.
//!
//! This guards both halves, because either alone is a lie:
//!  1. Static — EDITOR_HTML references the tree endpoints and defines the
//!     add-state / connect-transition actions, so the served UI actually calls
//!     the server rather than displaying placeholders.
//!  2. Functional — add-state and connect-transition round-trip through the
//!     AnimationTree graph on EditorState, driven through the real HTTP
//!     handlers, and are reflected by GET /api/animation/tree.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring guard.
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_animtree_endpoints() {
    assert!(
        EDITOR_HTML.contains("/api/animation/tree"),
        "EDITOR_HTML must GET /api/animation/tree to render the state machine"
    );
    assert!(
        EDITOR_HTML.contains("/api/animation/tree/state/add"),
        "EDITOR_HTML must POST /api/animation/tree/state/add"
    );
    assert!(
        EDITOR_HTML.contains("/api/animation/tree/transition/add"),
        "EDITOR_HTML must POST /api/animation/tree/transition/add"
    );
    assert!(
        EDITOR_HTML.contains("refreshAnimTree"),
        "EDITOR_HTML must define the tree refresh that renders server state"
    );
    assert!(
        EDITOR_HTML.contains("connectAnimTreeTransition"),
        "EDITOR_HTML must define the connect-transition action"
    );
    // The old shell hardcoded its node list; the states must now come from the
    // server, or the view is a placeholder again.
    assert!(
        EDITOR_HTML.contains("anim-tree-states"),
        "EDITOR_HTML must have a server-rendered states container"
    );
}

// ---------------------------------------------------------------------------
// (2) Functional round-trip through the live handlers.
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

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

/// The bead's acceptance: adding states and connecting a transition persists to
/// the AnimationTree graph and shows up in the served graph.
#[test]
fn editor_animtree_state_machine_wired_test() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));

    // Add two states.
    for name in ["Idle", "Run"] {
        let resp = http_post(
            port,
            "/api/animation/tree/state/add",
            &format!(r#"{{"name":"{name}"}}"#),
        );
        assert!(resp.contains("200 OK"), "add state {name} failed:\n{resp}");
    }

    // Connect Idle -> Run.
    let connect = http_post(
        port,
        "/api/animation/tree/transition/add",
        r#"{"from":"Idle","to":"Run"}"#,
    );
    assert!(connect.contains("200 OK"), "connect failed:\n{connect}");

    // The transition persisted to the real graph on EditorState.
    {
        let s = handle.state().lock().unwrap();
        let idle = s
            .anim_tree
            .node_id_by_name("Idle")
            .expect("Idle state should persist");
        let run = s
            .anim_tree
            .node_id_by_name("Run")
            .expect("Run state should persist");
        assert_eq!(s.anim_tree.node_count(), 2, "both states should persist");
        assert!(
            s.anim_tree.has_transition(idle, run),
            "connect-transition must persist Idle -> Run in the AnimationTree graph"
        );
    }

    // ...and the served graph reports it, which is what the UI renders.
    let graph = http_get(port, "/api/animation/tree");
    assert!(graph.contains("200 OK"), "GET tree failed:\n{graph}");
    assert!(
        graph.contains("\"Idle\"") && graph.contains("\"Run\""),
        "GET /api/animation/tree must list the added states, got:\n{graph}"
    );
    assert!(
        graph.contains("\"transitions\":[{"),
        "GET /api/animation/tree must report the transition, got:\n{graph}"
    );

    handle.stop();
}

/// Godot's state names are unique per state machine, and a transition needs
/// both endpoints to exist — the handlers must reject rather than corrupt the
/// graph.
#[test]
fn animtree_rejects_duplicate_state_and_unknown_transition() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));

    let first = http_post(port, "/api/animation/tree/state/add", r#"{"name":"Idle"}"#);
    assert!(first.contains("200 OK"), "first add should succeed:\n{first}");

    let dup = http_post(port, "/api/animation/tree/state/add", r#"{"name":"Idle"}"#);
    assert!(
        dup.contains("400"),
        "a duplicate state name must be rejected, got:\n{dup}"
    );

    let bad = http_post(
        port,
        "/api/animation/tree/transition/add",
        r#"{"from":"Idle","to":"Nope"}"#,
    );
    assert!(
        bad.contains("404"),
        "a transition to an unknown state must be rejected, got:\n{bad}"
    );

    {
        let s = handle.state().lock().unwrap();
        assert_eq!(s.anim_tree.node_count(), 1, "rejects must not add states");
        assert!(
            s.anim_tree.transitions().is_empty(),
            "a rejected transition must not land in the graph"
        );
    }

    handle.stop();
}

/// Removing a state drops the transitions that touch it, so the served graph
/// cannot show a dangling edge.
#[test]
fn animtree_remove_state_drops_its_transitions() {
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    thread::sleep(Duration::from_millis(100));

    for name in ["Idle", "Run"] {
        http_post(
            port,
            "/api/animation/tree/state/add",
            &format!(r#"{{"name":"{name}"}}"#),
        );
    }
    http_post(
        port,
        "/api/animation/tree/transition/add",
        r#"{"from":"Idle","to":"Run"}"#,
    );

    let removed = http_post(
        port,
        "/api/animation/tree/state/remove",
        r#"{"name":"Run"}"#,
    );
    assert!(removed.contains("200 OK"), "remove failed:\n{removed}");

    {
        let s = handle.state().lock().unwrap();
        assert_eq!(s.anim_tree.node_count(), 1, "Run should be gone");
        assert!(
            s.anim_tree.transitions().is_empty(),
            "removing a state must drop transitions touching it"
        );
    }

    handle.stop();
}
