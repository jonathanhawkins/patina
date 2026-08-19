//! pat-s3rrv.78 — `HTTPRequest.body_size_limit` and `HTTPRequest.download_file`.
//!
//! Godot ground truth: both are **node properties enforced by the node**, not by
//! the HTTP client — `HTTPRequest::_update_connection`'s body branch
//! (`scene/main/http_request.cpp:395-485`). That is why they are implementable
//! here with no transport change and no new dependency: an over-limit response
//! is a perfectly successful HTTP exchange that the node then refuses.
//!
//! 🔑 The assertion that carries this file is that `download_file` **empties the
//! signal body**. Upstream is
//! `if (file.is_valid()) { file->store_buffer(...) } else { body.append_array(chunk) }`
//! — an exclusive `else`. An implementation that writes the file *and* delivers
//! the bytes passes every "did the download work?" check a human would run, and
//! doubles the memory cost of exactly the large downloads the property exists to
//! avoid. Every test below that touches `download_file` asserts BOTH halves.

use std::sync::{Arc, Mutex};

use gdcore::id::ObjectId;
use gdobject::signal::Connection;
use gdplatform::http_request::{HttpResponse, HttpResultCode, RequestCompleted};
use gdscene::http_request_node::{
    deliver, DeliverySettings, RESULT_BODY_SIZE_LIMIT_EXCEEDED, RESULT_DOWNLOAD_FILE_CANT_OPEN,
};
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A body long enough that no limit in these tests is satisfied by accident.
const BODY: &[u8] = b"0123456789abcdef";

/// A private directory plus the path of a file inside it that does NOT exist yet.
///
/// ⚠️ Per-test rather than a shared name: nextest runs these in parallel in one
/// process, so a shared filename would let one test's write satisfy another
/// test's assertion — and two of the tests below assert a file is *absent*.
/// The returned `TempDir` must stay alive; dropping it deletes the directory.
fn scratch() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir.path().join("out.bin").to_string_lossy().into_owned();
    (dir, path)
}

fn ok_payload() -> RequestCompleted {
    RequestCompleted {
        result: 0,
        response_code: 200,
        headers: vec!["X-Patina-Test: yes".to_string()],
        body: BODY.to_vec(),
    }
}

fn limit(bytes: i64) -> DeliverySettings {
    DeliverySettings {
        body_size_limit: bytes,
        ..Default::default()
    }
}

fn to_file(path: &str) -> DeliverySettings {
    DeliverySettings {
        download_file: path.to_string(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- size limit

/// A body past the limit is refused — but the response is still described.
#[test]
fn an_over_limit_body_is_refused_with_code_and_headers_intact() {
    let got = deliver(ok_payload(), &limit(BODY.len() as i64 - 1));

    assert_eq!(
        got.result, RESULT_BODY_SIZE_LIMIT_EXCEEDED,
        "result 7, not a generic failure"
    );
    assert!(got.body.is_empty(), "the oversized body is dropped");
    // ⚠️ Upstream passes `response_code` and `response_headers` to `_defer_done`
    // and only the BODY is emptied, so a script can still tell a refused 200
    // from a refused 500.
    assert_eq!(got.response_code, 200, "the status code survives");
    assert_eq!(
        got.headers,
        vec!["X-Patina-Test: yes".to_string()],
        "the headers survive"
    );
}

/// 🔑 The boundary: upstream is `body_len > limit`, so a body of exactly the
/// limit is DELIVERED.
///
/// ⚠️ This is the test that fails on an off-by-one, and the reason the
/// over-limit test above uses `len - 1` rather than a round number: with a
/// `>=` comparison the two together still disagree, so one of them must break.
#[test]
fn a_body_of_exactly_the_limit_is_delivered() {
    let got = deliver(ok_payload(), &limit(BODY.len() as i64));

    assert_eq!(got.result, 0, "exactly at the limit is not over it");
    assert_eq!(got.body, BODY, "and the body arrives whole");
}

/// ⚠️ Negative means UNLIMITED, not "refuse everything" — and it is the DEFAULT
/// (`HTTPRequest.xml:242`), so getting this backwards breaks every project that
/// never touches the property.
#[test]
fn a_negative_limit_is_unlimited() {
    for unlimited in [-1, -64] {
        let got = deliver(ok_payload(), &limit(unlimited));
        assert_eq!(got.result, 0, "limit {unlimited} must not refuse anything");
        assert_eq!(got.body, BODY);
    }
    // The default is one of them.
    assert_eq!(DeliverySettings::default().body_size_limit, -1);
}

/// A payload that already failed is passed through untouched.
///
/// ⚠️ Upstream reaches the limit check only from `STATUS_BODY`; a transport
/// failure goes straight to `_defer_done`. Post-processing every payload would
/// overwrite a `CantConnect` with `BODY_SIZE_LIMIT_EXCEEDED` and send whoever
/// debugs it looking for a server that was never reached.
#[test]
fn a_transport_failure_is_not_post_processed() {
    let failed = RequestCompleted {
        result: HttpResultCode::CantConnect.as_godot_result_code(),
        response_code: 0,
        headers: Vec::new(),
        body: BODY.to_vec(),
    };
    let got = deliver(failed, &limit(1));

    assert_eq!(
        got.result,
        HttpResultCode::CantConnect.as_godot_result_code(),
        "the original failure must survive a limit that the body exceeds"
    );
}

// -------------------------------------------------------------- download_file

/// 🔑 The bytes go to the file **instead of** into the signal.
#[test]
fn download_file_writes_the_body_and_empties_the_signal() {
    let (_dir, path) = scratch();
    let got = deliver(ok_payload(), &to_file(&path));

    assert_eq!(got.result, 0, "a successful download still reports success");
    assert_eq!(
        std::fs::read(&path).expect("the file must exist"),
        BODY,
        "the body must reach the file"
    );
    // 🔑 The half an implementation that "works" still gets wrong.
    assert!(
        got.body.is_empty(),
        "with download_file set the signal body is EMPTY — upstream's `else` is \
         exclusive, so delivering both is a real behavioural difference, got {} \
         byte(s)",
        got.body.len()
    );
    assert_eq!(got.response_code, 200, "the status code still arrives");
}

/// An unwritable path is reported, not silently swallowed.
#[test]
fn an_unopenable_download_path_reports_cant_open() {
    // A parent directory that does not exist: `open` creates files, not dirs.
    let dir = tempfile::tempdir().expect("scratch dir");
    let path = dir
        .path()
        .join("absent")
        .join("out.bin")
        .to_string_lossy()
        .into_owned();
    assert!(
        !std::path::Path::new(&path).exists(),
        "fixture precondition: the path must not be openable"
    );

    let got = deliver(ok_payload(), &to_file(&path));

    assert_eq!(
        got.result, RESULT_DOWNLOAD_FILE_CANT_OPEN,
        "result 10, so a script can tell a bad path from a bad server"
    );
    assert!(got.body.is_empty());
    assert_eq!(got.response_code, 200, "the server still answered 200");
}

/// ⚠️ Ordering: the size check runs BEFORE the file is opened
/// (`http_request.cpp:408` precedes `:413`), so a refused download leaves no
/// stray empty file behind.
#[test]
fn an_over_limit_response_never_creates_the_download_file() {
    let (_dir, path) = scratch();

    let got = deliver(
        ok_payload(),
        &DeliverySettings {
            body_size_limit: BODY.len() as i64 - 1,
            download_file: path.clone(),
            ..Default::default()
        },
    );

    assert_eq!(got.result, RESULT_BODY_SIZE_LIMIT_EXCEEDED);
    assert!(
        !std::path::Path::new(&path).exists(),
        "opening the file before checking the limit would leave this empty file \
         behind on every rejected download"
    );
}

// ------------------------------------------------------- through the scene

/// Everything `request_completed` carried.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<(i64, i64, Vec<u8>)>>>);

fn watch_completed(tree: &mut SceneTree, node: NodeId) -> Captured {
    let log = Captured::default();
    let sink = log.0.clone();
    tree.connect_signal(
        node,
        "request_completed",
        Connection::with_callback(ObjectId::from_raw(781), "on_done", move |args| {
            let int = |i: usize| match args.get(i) {
                Some(Variant::Int(v)) => *v,
                other => panic!("arg {i} must be an int, got {other:?}"),
            };
            let body = match args.get(3) {
                Some(Variant::PackedByteArray(b)) => b.clone(),
                other => panic!("body must be a PackedByteArray, got {other:?}"),
            };
            sink.lock().unwrap().push((int(0), int(1), body));
            Variant::Nil
        }),
    );
    log
}

/// 🔑 The properties are honoured on the real emit path, not only in `deliver`.
///
/// ⚠️ A unit test of `deliver` alone passes even if `process_http_requests`
/// never calls it — which is the whole failure mode this bead exists to close.
/// This one drives a response through `MainLoop::step`, the way a scene does.
#[test]
fn the_frame_tick_honours_download_file_from_the_node_properties() {
    let (_dir, path) = scratch();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");
    // Authored the way a `.tscn` writes it.
    tree.get_node_mut(http)
        .expect("node")
        .set_property("download_file", Variant::String(path.clone()));

    let captured = watch_completed(&mut tree, http);
    tree.http_request_for(http).inject_response(HttpResponse {
        status_code: 200,
        headers: Default::default(),
        body: BODY.to_vec(),
        result: HttpResultCode::Success,
    });

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let seen = captured.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "one completion, got {seen:?}");
    let (result, code, body) = seen[0].clone();
    assert_eq!(result, 0);
    assert_eq!(code, 200);
    assert!(
        body.is_empty(),
        "the frame tick must apply download_file too, got {} byte(s)",
        body.len()
    );
    assert_eq!(
        std::fs::read(&path).expect("the file must exist"),
        BODY,
        "and the bytes must be on disk"
    );
}

/// The script-facing setters and getters round-trip.
///
/// ⚠️ A write path with no read path is how this kind of property silently
/// loses data: the getter must observe what the setter stored, or a script that
/// reads back its own configuration sees the default.
#[test]
fn the_script_setters_and_getters_round_trip() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let http = tree
        .add_child(root, Node::new("Http", "HTTPRequest"))
        .expect("add http node");

    let (_dir, path) = scratch();
    let src = format!(
        "extends Node\nvar limit = -99\nvar file = \"\"\nfunc run():\n    var r = get_node(\"Http\")\n    r.set_body_size_limit(4096)\n    r.set_download_file(\"{path}\")\n    self.limit = r.get_body_size_limit()\n    self.file = r.get_download_file()\n"
    );
    tree.attach_script(
        root,
        Box::new(gdscene::GDScriptNodeInstance::from_source(&src, root).expect("script parses")),
    );

    assert!(
        tree.call_script_method(root, "run", &[]).is_some(),
        "run() raised — a setter or getter did not resolve on the node"
    );

    let script_var = |name: &str| {
        tree.get_script(root)
            .expect("script attached")
            .get_property(name)
            .unwrap_or(Variant::Nil)
    };
    assert_eq!(
        script_var("limit"),
        Variant::Int(4096),
        "get_body_size_limit must observe what set_body_size_limit stored"
    );
    assert_eq!(
        script_var("file"),
        Variant::String(path.clone()),
        "get_download_file must observe what set_download_file stored"
    );
    // 🔑 And the emit path reads the SAME place the script wrote. Without this
    // the getters could round-trip through storage that `process_http_requests`
    // never consults, and every script-configured limit would be ignored.
    assert_eq!(
        gdscene::http_request_node::delivery_settings(tree.get_node(http).unwrap()).body_size_limit,
        4096,
        "the setter must write the node property that `delivery_settings` reads"
    );
}
