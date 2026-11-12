//! Regression test for pat-vl1mk.
//!
//! The served editor freezes the Chrome renderer within minutes of use because
//! its poll loop fired fire-and-forget `fetch`es for ~8 endpoints with no
//! single-in-flight guard and no request timeout. A network capture showed 138
//! requests all stuck 'pending' — in-flight requests piled up without resolving
//! or aborting and jammed the main thread.
//!
//! This is an app-level test. It (1) asserts the served `/editor` JS now guards
//! poll scheduling against overlapping requests and bounds each request with an
//! AbortController, and (2) boots the real server and hammers every polled
//! endpoint concurrently, asserting each request stays bounded (no endpoint
//! hangs and no handler/connection leak that would degrade latency over the
//! window). The harness mirrors editor_parity_test.rs so this file is
//! self-contained.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_server() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port)
}

/// One GET with a hard read timeout. Returns the wall-clock duration, or `None`
/// if the request did not complete within the timeout (i.e. the endpoint hung).
fn timed_get(port: u16, path: &str, timeout: Duration) -> Option<Duration> {
    let start = Instant::now();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .ok()?;
    let mut buf = Vec::new();
    // read_to_end returns Err on timeout; treat that as "hung / not bounded".
    match stream.read_to_end(&mut buf) {
        Ok(_) if !buf.is_empty() => Some(start.elapsed()),
        _ => None,
    }
}

fn http_get_body(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

/// (1) The served editor JS must guard against overlapping polls and bound each
/// request, so a slow endpoint can never leak a forever-pending fetch.
#[test]
fn editor_polling_guarded_in_served_js() {
    let (_h, port) = make_server();
    let html = http_get_body(port, "/editor");

    assert!(
        html.contains("schedulePoll"),
        "served editor must schedule polls through a single-in-flight guard (schedulePoll)"
    );
    assert!(
        html.contains("_pollInflight"),
        "poll scheduler must track in-flight state per endpoint to skip overlapping ticks"
    );
    assert!(
        html.contains("AbortController"),
        "api() must bound every request with an AbortController so fetches can't pile up pending"
    );
}

/// (2) Under sustained concurrent polling of every endpoint the live UI polls,
/// each request must stay bounded. Pre-fix, the bare /api/filesystem poll hung
/// forever and others queued behind a saturated connection pool; this asserts
/// no polled endpoint hangs and latency does not blow up over the window.
#[test]
fn editor_polling_bounded_inflight() {
    let (_h, port) = make_server();

    // Every param-less GET the served editor polls (see schedulePoll calls +
    // refreshViewport + pollRuntimeStatus in editor_ui.rs).
    let endpoints: &[&str] = &[
        "/api/scene",
        "/api/viewport/png",
        "/api/logs",
        "/api/scene/info",
        "/api/scene/tabs",
        "/api/monitors/frame_times",
        "/api/debug/state",
        "/api/profiler",
        "/api/filesystem",
        "/api/runtime/status",
        "/api/history",
    ];

    let per_request_timeout = Duration::from_secs(3);
    let window = Duration::from_secs(6);
    let max_acceptable = Duration::from_millis(1500);

    let hangs = Arc::new(AtomicU64::new(0));
    let slow = Arc::new(AtomicU64::new(0));
    let total = Arc::new(AtomicU64::new(0));

    let mut workers = Vec::new();
    for ep in endpoints {
        let ep = ep.to_string();
        let hangs = Arc::clone(&hangs);
        let slow = Arc::clone(&slow);
        let total = Arc::clone(&total);
        workers.push(thread::spawn(move || {
            let deadline = Instant::now() + window;
            while Instant::now() < deadline {
                total.fetch_add(1, Ordering::Relaxed);
                match timed_get(port, &ep, per_request_timeout) {
                    Some(d) => {
                        if d > max_acceptable {
                            slow.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    None => {
                        hangs.fetch_add(1, Ordering::Relaxed);
                    }
                }
                // ~10 Hz per endpoint: far more aggressive than the real UI, so
                // any pile-up/leak surfaces quickly.
                thread::sleep(Duration::from_millis(100));
            }
        }));
    }
    for w in workers {
        w.join().unwrap();
    }

    let total = total.load(Ordering::Relaxed);
    let hangs = hangs.load(Ordering::Relaxed);
    let slow = slow.load(Ordering::Relaxed);
    assert!(total > 0, "expected many polling requests, got none");
    assert_eq!(
        hangs, 0,
        "{hangs}/{total} polled requests hung past {per_request_timeout:?} (an endpoint never responded — connection-pool starvation)"
    );
    assert_eq!(
        slow, 0,
        "{slow}/{total} polled requests took longer than {max_acceptable:?} (latency degraded under sustained polling — handler/connection leak)"
    );
}
