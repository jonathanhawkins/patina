//! pat-s3rrv.78 — `cancel_request` must cancel *silently*.
//!
//! Godot ground truth: `HTTPRequest::cancel_request`
//! (`scene/main/http_request.cpp:186-210`) closes the client, sets
//! `requesting = false`, and emits nothing — there is no `emit_signal` anywhere
//! in the function. A cancelled request simply never reports.
//!
//! ⚠️ Patina's platform transport deliberately does the opposite: its `cancel`
//! records a `RequestFailed` response and clears `emitted`, so a scene-side
//! wrapper that just delegates would hand the script a `request_completed` that
//! Godot never sends. The script's handler would run, with a failure result,
//! for a request the script itself cancelled.
//!
//! 🔑 THE ASSERTION THAT MATTERS HERE IS AN ABSENCE. A test that checks only
//! `is_requesting() == false` afterwards passes against the spurious-callback
//! version, because that version does stop requesting — it just also emits. So
//! these tests step the loop well past the point where a completion would have
//! arrived and assert nothing was delivered.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gdcore::id::ObjectId;
use gdobject::signal::Connection;
use gdplatform::http_request::{HttpMethod, HttpResponse, HttpResultCode};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// Accepts a connection and answers only after `delay`.
///
/// ⚠️ The delay is the point: it keeps the request genuinely in flight while the
/// test cancels it, and then lets the transport finish AFTERWARDS. That second
/// half is what proves the late completion is suppressed as well as the
/// immediate one — a wrapper that only drops the queued failure would still let
/// this one through.
fn serve_after(delay: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            std::thread::sleep(delay);
            let body = "late";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(response.as_bytes());
            let _ = sock.flush();
        }
    });
    port
}

/// Records every `request_completed` the scene emits for `node`.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<i64>>>);

fn watch_completed(tree: &mut SceneTree, node: NodeId) -> Captured {
    let log = Captured::default();
    let sink = log.0.clone();
    tree.connect_signal(
        node,
        "request_completed",
        Connection::with_callback(ObjectId::from_raw(913), "on_done", move |args| {
            let result = match args.first() {
                Some(Variant::Int(v)) => *v,
                other => panic!("result must be an int, got {other:?}"),
            };
            sink.lock().unwrap().push(result);
            Variant::Nil
        }),
    );
    log
}

fn scene_with_http() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");
    (tree, http)
}

// ---------------------------------------------------------------------------

/// 🔑 The acceptance clause: a cancelled request never reports.
#[test]
fn a_cancelled_request_emits_no_request_completed() {
    let port = serve_after(Duration::from_millis(150));
    let (mut tree, http) = scene_with_http();
    let captured = watch_completed(&mut tree, http);

    let url = format!("http://127.0.0.1:{port}/slow");
    let started = tree.http_request_for(http).request(
        &url,
        HttpMethod::Get,
        Vec::new(),
        gdscene::http_request_node::RequestSettings::default(),
    );
    assert_eq!(
        started, 0,
        "the request must be accepted (OK) to be cancelled"
    );
    assert!(
        tree.http_request_for(http).is_requesting(),
        "the request must actually be in flight before we cancel it — if it \
         already finished, this test proves nothing about cancellation"
    );

    tree.http_request_for(http).cancel();

    assert!(
        !tree.http_request_for(http).is_requesting(),
        "cancel must stop the request"
    );

    // Step well past the server's 150ms delay, so the transport genuinely
    // completes DURING this loop and would emit if anything still could.
    let mut ml = MainLoop::new(tree);
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(5));
    }

    let seen = captured.0.lock().unwrap().clone();
    assert!(
        seen.is_empty(),
        "a cancelled request must emit nothing; got {} completion(s) with \
         result(s) {seen:?}. A failure result here means the platform's \
         recorded RequestFailed leaked through to the script.",
        seen.len(),
    );
}

/// ⚠️ Cancelling an idle node must not eat a response that already landed.
///
/// The platform `cancel` OVERWRITES the stored response, so a wrapper without
/// the `is_requesting` guard would silently discard a finished request whose
/// payload the frame tick had not drained yet — data loss that looks like a
/// request that simply never completed.
#[test]
fn cancelling_an_idle_node_does_not_discard_a_finished_response() {
    let (mut tree, http) = scene_with_http();

    tree.http_request_for(http).inject_response(HttpResponse {
        status_code: 200,
        headers: Default::default(),
        body: b"already here".to_vec(),
        result: HttpResultCode::Success,
    });

    // Not requesting — Godot's cancel_request returns immediately here
    // (http_request.cpp:189).
    assert!(!tree.http_request_for(http).is_requesting());
    tree.http_request_for(http).cancel();

    let done = tree.http_request_for(http).drain_completed();
    assert_eq!(
        done.len(),
        1,
        "the finished response must survive an idle cancel, got {done:?}"
    );
    assert_eq!(done[0].response_code, 200);
    assert_eq!(
        done[0].result, 0,
        "it must still report success — not the RequestFailed that an \
         unguarded cancel would have written over it"
    );
}

/// The node stays usable: a request issued after a cancel still completes.
///
/// `cancel` sets `emitted`, which is what suppresses the cancelled request's
/// late completion. This pins that the flag does not also mute the NEXT
/// request — `request()` clears it, and a wrapper that latched it some other
/// way would leave the node permanently silent.
#[test]
fn a_request_after_a_cancel_still_completes() {
    let slow = serve_after(Duration::from_millis(400));
    let (mut tree, http) = scene_with_http();
    let captured = watch_completed(&mut tree, http);

    tree.http_request_for(http).request(
        &format!("http://127.0.0.1:{slow}/slow"),
        HttpMethod::Get,
        Vec::new(),
        gdscene::http_request_node::RequestSettings::default(),
    );
    tree.http_request_for(http).cancel();

    let fast = serve_after(Duration::from_millis(0));
    let again = tree.http_request_for(http).request(
        &format!("http://127.0.0.1:{fast}/quick"),
        HttpMethod::Get,
        Vec::new(),
        gdscene::http_request_node::RequestSettings::default(),
    );
    assert_eq!(
        again, 0,
        "a new request after a cancel must be accepted, not ERR_BUSY"
    );

    let mut ml = MainLoop::new(tree);
    let mut arrived = false;
    for _ in 0..400 {
        ml.step(1.0 / 60.0);
        if !captured.0.lock().unwrap().is_empty() {
            arrived = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        arrived,
        "the request made after the cancel must still deliver \
         request_completed — the node must not be left permanently muted"
    );
    let seen = captured.0.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![0],
        "exactly one completion, RESULT_SUCCESS — the cancelled request must \
         not appear alongside it"
    );
}
