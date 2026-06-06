//! pat-no8k2: Editor viewport latency budget acceptance gate.
//!
//! Acceptance: 1000 sequential `GET /api/viewport` calls against a real
//! `editor_server` with a populated frame buffer yield a p99 below 50 ms on
//! the CI baseline machine. This guards against regressions that would push
//! the editor outside the interactive feel users expect.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use gdcore::math::Color;
use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::SceneTree;

/// p99 latency budget for `GET /api/viewport`, per pat-no8k2.
const VIEWPORT_P99_BUDGET_MS: u128 = 50;

/// Sample count for the budget measurement, per pat-no8k2.
const SAMPLE_COUNT: usize = 1000;

/// Viewport size used for the measurement. Bigger than the 4×4 fixture used
/// in the smoke tests so the BMP encoder, copy path, and HTTP framing get
/// exercised with a payload representative of what the editor actually serves.
const VIEWPORT_WIDTH: u32 = 128;
const VIEWPORT_HEIGHT: u32 = 128;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_server_with_frame() -> (EditorServerHandle, u16) {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let state = EditorState::new(tree);
    let handle = EditorServerHandle::start(port, state);

    // Push a representative-size frame buffer so `/api/viewport` has bytes
    // to return; without this the endpoint replies 404 and the latency
    // measurement would be meaningless.
    let fb = FrameBuffer::new(VIEWPORT_WIDTH, VIEWPORT_HEIGHT, Color::rgb(0.1, 0.2, 0.3));
    handle.update_frame(fb);

    // Wait for the listener to be ready.
    for _ in 0..40 {
        if TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(50),
        )
        .is_ok()
        {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    (handle, port)
}

/// Issue a single `GET /api/viewport` request and read the full response.
/// Returns the elapsed wall time for the round trip.
fn timed_viewport_request(port: u16) -> Duration {
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))
        .expect("connect to editor_server for latency probe");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");
    let req = b"GET /api/viewport HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let started = Instant::now();
    stream.write_all(req).expect("write viewport request");
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .expect("read viewport response");
    let elapsed = started.elapsed();
    // Sanity check so a regressed endpoint that returns 404 doesn't quietly
    // pass the latency budget by short-circuiting the encode path.
    let head_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response has header/body separator");
    let head = String::from_utf8_lossy(&buf[..head_end]);
    assert!(
        head.contains("200 OK"),
        "viewport request returned non-200 head: {head}"
    );
    elapsed
}

#[test]
fn viewport_p99_latency_under_budget_across_1000_calls() {
    let (handle, port) = start_server_with_frame();

    // Warm-up: prime the server, the TCP stack, and any first-call lazy work
    // (allocator / page faults) so the measurement reflects steady-state cost.
    for _ in 0..16 {
        let _ = timed_viewport_request(port);
    }

    let mut samples: Vec<Duration> = Vec::with_capacity(SAMPLE_COUNT);
    let overall = Instant::now();
    for _ in 0..SAMPLE_COUNT {
        samples.push(timed_viewport_request(port));
    }
    let overall_elapsed = overall.elapsed();

    handle.stop();

    samples.sort();
    let p50 = samples[SAMPLE_COUNT / 2];
    let p90 = samples[(SAMPLE_COUNT as f64 * 0.90) as usize];
    let p99 = samples[(SAMPLE_COUNT as f64 * 0.99) as usize];
    let max = *samples.last().unwrap();

    eprintln!(
        "viewport latency: p50={p50:?} p90={p90:?} p99={p99:?} max={max:?} \
         overall {overall_elapsed:?} for {SAMPLE_COUNT} samples"
    );

    assert!(
        p99.as_millis() < VIEWPORT_P99_BUDGET_MS,
        "/api/viewport p99 = {p99:?} exceeds {VIEWPORT_P99_BUDGET_MS} ms budget; \
         p50={p50:?} p90={p90:?} max={max:?}"
    );
}
