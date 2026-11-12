//! pat-s3rrv.78 — the HTTPRequest settings and `cancel_request` reach a script.
//!
//! `HttpRequestNode::cancel` has existed and been correct the whole time, and
//! `timeout` / `max_redirects` are now carried into the transport config — but
//! none of it had a route from GDScript. A cancel button in a game could not
//! cancel anything, because the only caller was Rust.
//!
//! ⚠️ Every assertion here reads the NODE after driving it through a script.
//! Asserting on the script's own return values would pass against a build where
//! the arms stored the value in a script-local and never touched the node —
//! which is the failure this closes.
//!
//! Ground truth: `scene/main/http_request.cpp:186-210` (cancel emits nothing),
//! `:255` (a negative redirect limit means unlimited), `:591`, `:616`.

use std::net::TcpListener;
use std::time::Duration;

use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// Accepts a connection and then never answers, so a request stays in flight
/// long enough to be cancelled.
///
/// ⚠️ The listener is *moved into* the thread and held, not dropped: dropping it
/// closes the socket, the client sees EOF, and the request completes on its own
/// — which would make the cancellation assertion pass for the wrong reason.
fn serve_never() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let _held = listener.accept();
        std::thread::sleep(Duration::from_secs(30));
    });
    port
}

fn scene(port: u16) -> (MainLoop, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, Node::new("Main", "Node"))
        .expect("add main");
    let http = tree
        .add_child(main, Node::new("Http", "HTTPRequest"))
        .expect("add http node");

    let src = format!(
        "extends Node\nvar seen_timeout = -1.0\nvar seen_redirects = 0\nvar fired = 0\nfunc configure():\n    var r = get_node(\"Http\")\n    r.set_timeout(3.5)\n    r.set_max_redirects(-1)\n    self.seen_timeout = r.get_timeout()\n    self.seen_redirects = r.get_max_redirects()\nfunc go():\n    get_node(\"Http\").request(\"http://127.0.0.1:{port}/slow\")\nfunc stop():\n    get_node(\"Http\").cancel_request()\nfunc _on_done(result, response_code, headers, body):\n    self.fired = self.fired + 1\n"
    );
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(&src, main).expect("script parses")),
    );
    tree.connect_signal(
        http,
        "request_completed",
        Connection::new(main.object_id(), "_on_done"),
    );
    (MainLoop::new(tree), main, http)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// The setters reach the node, and the getters read it back.
#[test]
fn the_settings_round_trip_through_a_script() {
    let (mut ml, main, http) = scene(serve_never());
    assert!(
        ml.tree_mut()
            .call_script_method(main, "configure", &[])
            .is_some(),
        "configure() raised — most likely one of the setters did not resolve on the node",
    );

    // Read the NODE, not the script: this is what separates "the arm wrote the
    // node" from "the arm invented a script-local".
    //
    // ⚠️ Copied out inside a block so the `&mut ml` borrow ends before the
    // `&ml` reads below.
    let node = ml.tree().get_node(http).expect("http node");
    assert_eq!(
        node.get_property("timeout"),
        Variant::Float(3.5),
        "set_timeout must land on the NODE PROPERTY — the same place a .tscn \
         writes it and `r.timeout = 3.5` writes it",
    );
    assert_eq!(
        node.get_property("max_redirects"),
        Variant::Int(-1),
        "set_max_redirects stores verbatim; -1 means unlimited, not none",
    );

    // …and the getters returned the same values into the script.
    assert_eq!(script_var(&ml, main, "seen_timeout"), Variant::Float(3.5));
    assert_eq!(script_var(&ml, main, "seen_redirects"), Variant::Int(-1));
}

/// 🔑 A script can cancel, and cancelling reports nothing.
#[test]
fn a_script_cancel_request_stops_the_request_silently() {
    let (mut ml, main, http) = scene(serve_never());

    assert!(
        ml.tree_mut().call_script_method(main, "go", &[]).is_some(),
        "go() raised",
    );
    // ⚠️ The request must actually be in flight, or this proves nothing about
    // cancellation — it would just be asserting that an idle node is quiet.
    assert!(
        ml.tree_mut().http_request_for(http).is_requesting(),
        "the fixture server must keep the request open for the cancel to matter",
    );

    assert!(
        ml.tree_mut()
            .call_script_method(main, "stop", &[])
            .is_some(),
        "stop() raised — `cancel_request` did not resolve on the node",
    );
    assert!(
        !ml.tree_mut().http_request_for(http).is_requesting(),
        "the node must be idle after a script cancel",
    );

    // Godot's cancel emits NOTHING — not a failure result. Step well past the
    // point where a completion would have been delivered.
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        script_var(&ml, main, "fired"),
        Variant::Int(0),
        "a cancelled request must never report; a failure payload here is the \
         transport's recorded cancellation leaking through",
    );
}
