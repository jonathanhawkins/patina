//! pat-s3rrv.1: the editor graph is built FROM the machine a scene already has.
//!
//! # The gap this closes
//!
//! The AnimationTree graph could only ever be *authored*. Nothing built an
//! `AnimTreeGraph` from an `AnimationNode` tree, so opening a project whose
//! scene already carried an `AnimationTree` showed an empty canvas.
//!
//! That was merely missing until `POST /api/animation/tree/apply` landed in the
//! previous increment. With a write path and no read path, a user could open a
//! scene, see nothing, press apply, and **replace a real state machine with an
//! empty one** — the editor reporting 200 the whole way. This is the read half.

use gdscene::animation_tree::{AnimationLeafOptions, AnimationNode, StateMachine, StateTransition};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for _ in 0..50 {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            return s;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("editor server never accepted a connection on {port}");
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut s = connect_with_retry(port);
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).expect("write");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn http_get(port: u16, path: &str) -> String {
    let mut s = connect_with_retry(port);
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).expect("write");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn leaf(name: &str) -> AnimationNode {
    AnimationNode::Animation {
        name: name.to_string(),
        position: 0.0,
        opts: AnimationLeafOptions::default(),
    }
}

/// A scene whose `AnimationTree` already holds a real machine — the situation
/// the editor could not see at all.
fn boot_with_machine() -> (EditorServerHandle, u16) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let anim = tree
        .add_child(main, Node::new("Tree", "AnimationTree"))
        .unwrap();

    let mut machine = StateMachine::new();
    machine.states.insert("run".into(), leaf("run"));
    machine.states.insert("idle".into(), leaf("idle_loop"));
    machine.current = "idle".into();
    machine.transitions.push(StateTransition {
        from: "idle".into(),
        to: "run".into(),
        crossfade: 0.25,
        ..StateTransition::default()
    });

    let mut anim_tree = gdscene::animation_tree::AnimationTree::new();
    anim_tree.set_root(AnimationNode::StateMachine(machine));
    tree.attach_animation_tree(anim, anim_tree)
        .expect("the AnimationTree node accepts a tree");

    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// **The acceptance case.** What the scene holds is what the editor shows.
#[test]
fn loading_populates_the_graph_from_the_scene() {
    let (handle, port) = boot_with_machine();

    // Before: the canvas is blank even though the scene has two states.
    let before = http_get(port, "/api/animation/tree");
    assert!(
        before.contains(r#""states":[]"#),
        "precondition — the graph starts empty; got:\n{before}"
    );

    let resp = http_post(port, "/api/animation/tree/load", r#"{"node":"Tree"}"#);
    assert!(
        resp.contains("200 OK"),
        "POST /api/animation/tree/load should return 200, got:\n{resp}"
    );
    assert!(
        resp.contains(r#""states":2"#) && resp.contains(r#""transitions":1"#),
        "both states and the edge must load; got:\n{resp}"
    );

    let after = http_get(port, "/api/animation/tree");
    assert!(
        after.contains(r#""name":"idle""#) && after.contains(r#""name":"run""#),
        "the graph the client polls must now carry both states:\n{after}"
    );
    assert!(
        after.contains(r#""xfade":0.25"#),
        "and the transition's crossfade, which is authored data:\n{after}"
    );
    // ⚠️ `idle` plays the clip `idle_loop`. A state and its clip are independent
    // in Godot, and before this the node name was the only place a clip could be
    // recorded — so loading and re-applying used to RENAME the clip to `idle`.
    assert!(
        after.contains(r#""animation":"idle_loop""#),
        "the clip name must survive where the param route can edit it:\n{after}"
    );
    assert!(
        after.contains(r#""current_state":"idle""#),
        "the machine's starting state drives the editor highlight:\n{after}"
    );

    drop(handle);
}

/// Load then apply must be a no-op on the scene. This is the property that
/// makes the pair safe: opening a scene and saving it back without touching
/// anything cannot damage the machine.
#[test]
fn loading_then_applying_preserves_the_machine() {
    let (handle, port) = boot_with_machine();

    assert!(http_post(port, "/api/animation/tree/load", r#"{"node":"Tree"}"#).contains("200 OK"));
    let resp = http_post(port, "/api/animation/tree/apply", r#"{"node":"Tree"}"#);

    assert!(resp.contains("200 OK"), "apply should succeed:\n{resp}");
    assert!(
        resp.contains(r#""states":2"#),
        "the round trip must keep both states — a state lost here is a state \
         erased from the user's scene:\n{resp}"
    );
    assert!(
        resp.contains(r#""transitions":1"#),
        "and the transition:\n{resp}"
    );

    drop(handle);
}

/// A node with no `AnimationTree` attached is a 404, not an empty graph
/// reported as success — the editor must not show a blank canvas for a target
/// it failed to read.
#[test]
fn loading_from_a_node_without_a_tree_is_a_404() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    tree.add_child(main, Node::new("Tree", "AnimationTree"))
        .unwrap();
    let port = free_port();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(port, "/api/animation/tree/load", r#"{"node":"Tree"}"#);
    assert!(resp.contains("404"), "expected 404, got:\n{resp}");
    assert!(!resp.contains(r#""ok":true"#), "and no success:\n{resp}");

    drop(handle);
}

/// Loading from something that is not an `AnimationTree` is refused, the same
/// way applying to one is.
#[test]
fn loading_from_a_non_animation_tree_is_refused() {
    let (handle, port) = boot_with_machine();

    let resp = http_post(port, "/api/animation/tree/load", r#"{"node":"Main"}"#);
    assert!(
        resp.contains("400"),
        "reading a Node2D as an AnimationTree must be refused, got:\n{resp}"
    );

    drop(handle);
}

// ---------------------------------------------------------------------------
// Static wiring: a route nothing in the UI calls is the same dead end the graph
// model itself was for three increments.
// ---------------------------------------------------------------------------

/// The served editor must offer both directions. `apply` landed with no control
/// at all, so the only way to reach either route was a hand-written `curl`.
#[test]
fn the_served_editor_offers_load_and_apply() {
    let html = gdeditor::editor_ui::EDITOR_HTML;

    assert!(
        html.contains("/api/animation/tree/load"),
        "EDITOR_HTML must POST /api/animation/tree/load"
    );
    assert!(
        html.contains("/api/animation/tree/apply"),
        "EDITOR_HTML must POST /api/animation/tree/apply"
    );
    for id in ["anim-tree-load", "anim-tree-apply"] {
        assert!(
            html.contains(&format!(r#"id="{id}""#)),
            "the {id} button must exist in the toolbar"
        );
        // ⚠️ A button with no listener renders and does nothing — the exact
        // shape of "wired but inert" this bead has already produced once.
        assert!(
            html.contains(&format!("getElementById('{id}')")),
            "the {id} button must be looked up and bound"
        );
    }
    assert!(
        html.contains("loadAnimTreeFromScene") && html.contains("applyAnimTreeToScene"),
        "both handlers must be defined"
    );
}
