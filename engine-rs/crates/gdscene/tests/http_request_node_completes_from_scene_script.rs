//! Acceptance test for pat-s3rrv.78: an `HTTPRequest` node in a scene, driven
//! by a script, completes and delivers `request_completed`.
//!
//! Godot ground truth: `HTTPRequest` is a Node. A script calls `request(url)`,
//! the node polls its client each frame, and
//! `request_completed(result, response_code, headers, body)` fires on the
//! connected handler — with no engine-side driving from the host program.
//!
//! ⚠️ The gap this closes is the worst shape a gap takes. `HTTPRequest` **was**
//! registered in ClassDB and `gdplatform::http_request` is 602 working lines,
//! but `grep HTTPRequest crates/gdscene/src crates/gdscript-interop/src` found
//! **zero** hits: a `.tscn` node of that type loaded, answered `get_class()`
//! correctly, and did nothing at all. Everything except the behaviour said it
//! worked.
//!
//! The server here is a plain `TcpListener` on 127.0.0.1 with an ephemeral port
//! — no external network, and the port cannot collide with a parallel test.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gdcore::id::ObjectId;
use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// What the fixture server answers with.
const BODY: &str = "hello from the test server";

/// Serves exactly one request, then stops.
///
/// Returns the bound port. The response carries a custom header so the test can
/// prove headers survive the trip rather than only checking the status line.
fn serve_once() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            // Read the request head; the client sends no body for a GET.
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Patina-Test: yes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                BODY.len(),
                BODY
            );
            let _ = sock.write_all(response.as_bytes());
            let _ = sock.flush();
        }
    });
    port
}

/// Everything `request_completed` carried, captured from the scene's signal.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<(i64, i64, Vec<String>, Vec<u8>)>>>);

fn watch_completed(tree: &mut SceneTree, node: NodeId) -> Captured {
    let log = Captured::default();
    let sink = log.0.clone();
    tree.connect_signal(
        node,
        "request_completed",
        Connection::with_callback(ObjectId::from_raw(701), "on_done", move |args| {
            let int = |i: usize| match args.get(i) {
                Some(Variant::Int(v)) => *v,
                other => panic!("arg {i} must be an int, got {other:?}"),
            };
            let headers = match args.get(2) {
                Some(Variant::PackedStringArray(h)) => h.clone(),
                other => panic!("headers must be a PackedStringArray, got {other:?}"),
            };
            let body = match args.get(3) {
                Some(Variant::PackedByteArray(b)) => b.clone(),
                other => panic!("body must be a PackedByteArray, got {other:?}"),
            };
            sink.lock().unwrap().push((int(0), int(1), headers, body));
            Variant::Nil
        }),
    );
    log
}

/// A scene whose root script drives an `HTTPRequest` child, as a `.tscn` would
/// declare it.
fn scene_with_http(port: u16) -> (MainLoop, NodeId, NodeId, Captured) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree
        .add_child(root, Node::new("Main", "Node"))
        .expect("add main");
    let http = tree
        .add_child(main, Node::new("Http", "HTTPRequest"))
        .expect("add http node");

    let captured = watch_completed(&mut tree, http);

    // The script both STARTS the request and observes the completion, which is
    // the flow a project writes. `_on_done` proves the signal reaches GDScript;
    // the Rust recorder above captures the payload precisely enough to assert
    // on the body bytes.
    let src = format!(
        "extends Node\nvar done = 0\nvar code = -1\nfunc run():\n    get_node(\"Http\").request(\"http://127.0.0.1:{port}/hello\")\nfunc _on_done(result, response_code, headers, body):\n    self.done = self.done + 1\n    self.code = response_code\n"
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

    (MainLoop::new(tree), main, http, captured)
}

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// Steps the loop until `pred` holds or the budget runs out.
///
/// ⚠️ Bounded, not unbounded: the transport runs on a background thread, so a
/// broken wiring would hang a `loop {}` forever instead of failing. The budget
/// is generous enough for a localhost round trip and short enough to fail fast.
fn step_until(ml: &mut MainLoop, mut pred: impl FnMut(&MainLoop) -> bool) -> bool {
    for _ in 0..400 {
        ml.step(1.0 / 60.0);
        if pred(ml) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

// ---------------------------------------------------------------------------

/// 🔑 The whole chain: script → `request()` → frame tick → `request_completed`.
#[test]
fn http_request_node_completes_from_scene_script() {
    let port = serve_once();
    let (mut ml, main, _http, captured) = scene_with_http(port);

    // `run()` is the script calling `request()` on its HTTPRequest child.
    assert!(
        ml.tree_mut().call_script_method(main, "run", &[]).is_some(),
        "run() raised — most likely `request` did not resolve on the node at all"
    );

    let arrived = step_until(
        &mut ml,
        |ml| matches!(script_var(ml, main, "done"), Variant::Int(n) if n > 0),
    );
    assert!(
        arrived,
        "request_completed never reached the script within the step budget — \
         the node polls on the frame tick, so this fails if nothing drives it"
    );

    // The script saw it…
    assert_eq!(
        script_var(&ml, main, "code"),
        Variant::Int(200),
        "the GDScript handler must receive the real status code"
    );

    // …and the payload is what Godot promises.
    let seen = captured.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "exactly one completion, got {}", seen.len());
    let (result, code, headers, body) = seen[0].clone();
    assert_eq!(result, 0, "RESULT_SUCCESS");
    assert_eq!(code, 200);
    assert_eq!(
        String::from_utf8_lossy(&body),
        BODY,
        "the body bytes must survive the trip intact"
    );
    assert!(
        headers.iter().any(|h| h.starts_with("X-Patina-Test:")),
        "response headers must be delivered, got {headers:?}"
    );
}

/// ⚠️ A second request while one is in flight is refused with `ERR_BUSY`, not
/// silently queued.
///
/// Godot returns `ERR_BUSY` (44) rather than cancelling or stacking, and a
/// script that ignores the return value would otherwise believe both were sent.
#[test]
fn a_second_request_while_busy_is_refused() {
    let port = serve_once();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");

    let url = format!("http://127.0.0.1:{port}/hello");
    let first = tree.http_request_for(http).request(
        &url,
        gdplatform::http_request::HttpMethod::Get,
        Vec::new(),
        gdscene::http_request_node::RequestSettings::default(),
    );
    let second = tree.http_request_for(http).request(
        &url,
        gdplatform::http_request::HttpMethod::Get,
        Vec::new(),
        gdscene::http_request_node::RequestSettings::default(),
    );

    assert_eq!(first, 0, "the first request is accepted (OK)");
    assert_eq!(
        second, 44,
        "the second is refused with ERR_BUSY, not queued"
    );
}

/// An unsupported method ordinal is refused rather than quietly downgraded.
///
/// ⚠️ Godot has nine methods; this transport has six. OPTIONS/TRACE/CONNECT map
/// to nothing, and substituting GET would issue a request that looks successful
/// and does the wrong thing to whatever is on the other end.
#[test]
fn an_unsupported_method_ordinal_is_refused() {
    use gdscene::http_request_node::method_from_int;

    // GET 0, HEAD 1, POST 2, PUT 3, DELETE 4, OPTIONS 5, TRACE 6, CONNECT 7,
    // PATCH 8 (core/io/http_client.h:118-126).
    for supported in [0, 1, 2, 3, 4, 8] {
        assert!(
            method_from_int(supported).is_some(),
            "ordinal {supported} is one Godot defines and this transport has"
        );
    }
    for unsupported in [5, 6, 7] {
        assert!(
            method_from_int(unsupported).is_none(),
            "ordinal {unsupported} has no transport variant and must NOT fall \
             back to GET"
        );
    }
    // ⚠️ PATCH is 8, not 6 — 6 is TRACE. Counting this crate's own enum instead
    // of reading the upstream header gets it wrong, because that enum omits
    // three of Godot's nine.
    assert!(method_from_int(6).is_none(), "6 is TRACE, not PATCH");
}
