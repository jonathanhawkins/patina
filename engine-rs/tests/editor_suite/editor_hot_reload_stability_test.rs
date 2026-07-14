//! pat-d0mjj: Acceptance gate for editor stability under 1000 hot-reload cycles.
//!
//! Acceptance: this test runs 1000 scene-reload cycles and asserts:
//!   1. No leaked file descriptors (process FD count is bounded across cycles).
//!   2. No leaked sockets (every HTTP connection is closed by the handler;
//!      covered by the same FD census since sockets are FDs on Unix).
//!   3. No growing scene-ID table (the editor's scene tree node count stays
//!      constant across reloads — each load replaces the tree rather than
//!      accumulating into it).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn http_raw(port: u16, request: &str) -> String {
    let mut stream =
        TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect to editor server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    let _ = stream.read_to_string(&mut resp);
    resp
}

fn http_get(port: u16, path: &str) -> String {
    http_raw(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_raw(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn status_code(response: &str) -> u16 {
    let first = response.lines().next().unwrap_or("");
    first
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn extract_body(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

/// Count the entries the OS currently exposes as open file descriptors for
/// this process. Linux uses `/proc/self/fd`; macOS uses `/dev/fd`. Returns
/// `None` on platforms that don't expose either (the test skips its FD
/// assertions in that case rather than producing a false negative).
fn count_open_fds() -> Option<usize> {
    let path = if cfg!(target_os = "linux") {
        "/proc/self/fd"
    } else if cfg!(target_os = "macos") {
        "/dev/fd"
    } else {
        return None;
    };
    std::fs::read_dir(path)
        .ok()
        .map(|iter| iter.filter_map(|e| e.ok()).count())
}

fn count_nodes_in_scene_json(scene_json: &serde_json::Value) -> usize {
    fn walk(v: &serde_json::Value) -> usize {
        let mut count = 1;
        if let Some(children) = v.get("children").and_then(|c| c.as_array()) {
            for child in children {
                count += walk(child);
            }
        }
        count
    }
    if let Some(arr) = scene_json["nodes"].as_array() {
        arr.iter().map(walk).sum()
    } else if scene_json["nodes"].is_object() {
        walk(&scene_json["nodes"])
    } else {
        0
    }
}

fn wait_for_server(port: u16) {
    for _ in 0..40 {
        if TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn editor_survives_1000_hot_reload_cycles_without_leaks() {
    // Start an in-process editor server.
    let port = free_port();
    let state = EditorState::new(SceneTree::new());
    let handle = EditorServerHandle::start(port, state);
    wait_for_server(port);

    // Seed a small, valid .tscn file to reload over and over.
    let tmp = tempfile::tempdir().expect("tempdir");
    let scene_path = tmp.path().join("hot_reload_target.tscn");
    let scene_body = "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n";
    std::fs::write(&scene_path, scene_body).expect("seed .tscn");

    // Warm up: load once so any one-shot caches / lazy init don't appear as
    // a "leak" in the post-baseline sample.
    let load_body = serde_json::json!({ "path": scene_path.to_string_lossy() }).to_string();
    let warm = http_post(port, "/api/scene/load", &load_body);
    assert_eq!(
        status_code(&warm),
        200,
        "warmup load must succeed; got {warm}"
    );

    // Establish the FD baseline AFTER warmup so any first-use lazy
    // allocations (thread spawns, log buffers, etc.) are already counted.
    let baseline_fds = count_open_fds();

    // Capture the node count after a successful load. This is the "scene-ID
    // table" baseline — every subsequent reload must produce the same count.
    let initial_scene = http_get(port, "/api/scene");
    assert_eq!(status_code(&initial_scene), 200);
    let initial_body = extract_body(&initial_scene);
    let initial_json: serde_json::Value =
        serde_json::from_str(initial_body).expect("scene JSON parses");
    let initial_node_count = count_nodes_in_scene_json(&initial_json);
    assert!(
        initial_node_count >= 2,
        "expected root + scene-root nodes after load; got {initial_node_count}: {initial_body}"
    );

    // Sample at intervals so a quadratic-time growth would be obvious in the
    // panic message even if the final tail looks fine.
    let mut sampled_counts: Vec<(usize, usize)> = Vec::new(); // (iter, node count)
    let mut sampled_fds: Vec<(usize, Option<usize>)> = Vec::new();

    const CYCLES: usize = 1000;
    const SAMPLE_EVERY: usize = 100;

    for i in 0..CYCLES {
        let resp = http_post(port, "/api/scene/load", &load_body);
        let status = status_code(&resp);
        assert_eq!(
            status, 200,
            "iteration {i}: /api/scene/load must succeed; got HTTP {status}: {resp}"
        );

        if i % SAMPLE_EVERY == 0 {
            let scene = http_get(port, "/api/scene");
            assert_eq!(status_code(&scene), 200, "iter {i}: /api/scene must succeed");
            let v: serde_json::Value =
                serde_json::from_str(extract_body(&scene)).expect("scene JSON parses");
            sampled_counts.push((i, count_nodes_in_scene_json(&v)));
            sampled_fds.push((i, count_open_fds()));
        }
    }

    // Final samples for the post-loop assertion.
    let final_scene = http_get(port, "/api/scene");
    let final_body = extract_body(&final_scene);
    let final_json: serde_json::Value =
        serde_json::from_str(final_body).expect("final scene JSON parses");
    let final_node_count = count_nodes_in_scene_json(&final_json);
    let final_fds = count_open_fds();

    // ---------- Assertion 1: scene-ID table does not grow. ----------
    for (iter, count) in &sampled_counts {
        assert_eq!(
            *count, initial_node_count,
            "scene-ID table grew at iteration {iter}: initial={initial_node_count}, sampled={count}"
        );
    }
    assert_eq!(
        final_node_count, initial_node_count,
        "scene-ID table grew over 1000 cycles: initial={initial_node_count}, final={final_node_count}"
    );

    // ---------- Assertion 2: FD count is bounded. ----------
    if let (Some(baseline), Some(final_count)) = (baseline_fds, final_fds) {
        // A small drift is tolerated because the server reuses worker threads
        // and the OS may keep epoll/kqueue descriptors around between samples.
        // What we DO NOT tolerate is a count that grows roughly proportional
        // to the number of cycles.
        const MAX_FD_DRIFT: usize = 32;
        let drift = final_count.saturating_sub(baseline);
        assert!(
            drift <= MAX_FD_DRIFT,
            "file descriptor leak detected after {CYCLES} cycles: baseline={baseline}, final={final_count}, drift={drift}"
        );

        // Also verify the mid-test sample is bounded — if FDs were leaking
        // linearly, mid-test counts would already exceed MAX_FD_DRIFT.
        for (iter, sample) in &sampled_fds {
            if let Some(count) = sample {
                let mid_drift = count.saturating_sub(baseline);
                assert!(
                    mid_drift <= MAX_FD_DRIFT,
                    "FD count climbing during run: baseline={baseline}, iter={iter}, count={count}, drift={mid_drift}"
                );
            }
        }
    }
    // If the OS doesn't expose /proc/self/fd or /dev/fd, the FD assertion is
    // skipped but the scene-ID assertion above still proves the cycle is
    // doing real work and not silently no-oping.

    handle.stop();
}

/// Companion test: a single load cycle behaves correctly and increases the
/// node count over the empty baseline. Without this, a regression that made
/// /api/scene/load a no-op would silently pass the 1000-cycle test.
#[test]
fn single_hot_reload_increases_node_count() {
    let port = free_port();
    let state = EditorState::new(SceneTree::new());
    let handle = EditorServerHandle::start(port, state);
    wait_for_server(port);

    // Before loading, the scene tree is just the root node.
    let pre = http_get(port, "/api/scene");
    let pre_body = extract_body(&pre);
    let pre_json: serde_json::Value = serde_json::from_str(pre_body).expect("scene JSON parses");
    let pre_count = count_nodes_in_scene_json(&pre_json);
    assert!(pre_count >= 1, "expected at least the root node; got {pre_count}: {pre_body}");

    let tmp = tempfile::tempdir().expect("tempdir");
    let scene_path = tmp.path().join("single_reload.tscn");
    std::fs::write(
        &scene_path,
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n",
    )
    .unwrap();

    let body = serde_json::json!({ "path": scene_path.to_string_lossy() }).to_string();
    let resp = http_post(port, "/api/scene/load", &body);
    assert_eq!(status_code(&resp), 200, "load must succeed; got {resp}");

    let post = http_get(port, "/api/scene");
    let post_body = extract_body(&post);
    let post_json: serde_json::Value = serde_json::from_str(post_body).expect("scene JSON parses");
    let post_count = count_nodes_in_scene_json(&post_json);
    assert!(
        post_count > pre_count,
        "loading must add at least one node; pre={pre_count}, post={post_count}, body={post_body}"
    );

    handle.stop();
}
