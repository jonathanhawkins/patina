//! pat-mgixk.14 item 2 acceptance — the Profiler tab is fed by the running
//! game, and Video RAM reports what is actually resident.
//!
//! Before this, `EditorState::profiler_frames` had exactly ONE producer: the
//! test-only `POST /api/profiler/record`, which no client ever calls (it
//! appears in the server's route table and in two gdeditor tests, nowhere
//! else). Nothing in the play tick recorded a frame, so the Profiler tab stayed
//! empty through every Play no matter how long you left it running. Video RAM
//! was a string literal — `icon.svg` 262144, `font_default` 65536, `viewport`
//! 3686400 — reported identically for every project, whether or not it owned a
//! file by either name.
//!
//! NOTE ON THE FILE NAME: the acceptance calls its test
//! `profiler_records_frames_from_play_loop`, which is kept on the test
//! FUNCTION; the file is shorter so the `--test` command stays inside the
//! length the verify lane has truncated before (see `anim_audio_track.rs`).

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdrender2d::texture::Texture2D;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

/// `_process` calls `busy()` every frame, and `busy()` does enough real work to
/// be measurable.
///
/// ⚠️ The loop is load-bearing. `/api/profiler` formats `total_ms` with
/// `{:.2}`, so a frame faster than 0.005 ms serialises as `0.00` and the
/// acceptance's `total_ms > 0` would fail on a feature that works. A few
/// hundred interpreted iterations put the frame well clear of that, and
/// "`_process` calls a function `busy()`" is what the acceptance asks for
/// anyway.
const MAIN_GD: &str = "\
extends Node2D
var total = 0
func busy():
    var i = 0
    var acc = 0
    while i < 200:
        acc = acc + i
        i = i + 1
    return acc
func _process(delta):
    self.total = self.busy()
";

/// The texture the scene references, and the size its Video RAM row must
/// report: 48 x 32 x 4 bytes (RGBA8).
const TEX_W: u32 = 48;
const TEX_H: u32 = 32;
const TEX_BYTES: u64 = (TEX_W as u64) * (TEX_H as u64) * 4;

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

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http_request_str(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http_request_str(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// A scene with a scripted root and a Sprite2D that references a texture, plus
/// that texture already resident in the editor's cache.
///
/// ⚠️ The texture is inserted through `TextureCache::insert` — the same public
/// entry the editor's own render path fills the cache through — rather than by
/// writing a PNG and provoking a render. That keeps the Video RAM assertion
/// about what the endpoint REPORTS rather than about whether a render happened
/// to run before the poll.
fn server() -> (EditorServerHandle, u16, tempfile::TempDir) {
    gdobject::class_db::register_2d_classes();

    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.gd"), MAIN_GD).expect("write script");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("res://main.gd".to_string()));
    let main_id = tree.add_child(root, main).unwrap();
    let mut spr = Node::new("Spr", "Sprite2D");
    spr.set_property("texture", Variant::String("res://icon.png".to_string()));
    tree.add_child(main_id, spr).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);
    state.texture_cache.insert(
        "res://icon.png",
        Texture2D::solid(TEX_W, TEX_H, gdcore::math::Color::new(1.0, 0.0, 0.0, 1.0)),
    );

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// Total `calls` recorded for `name` across every captured frame.
///
/// Summed, because `functions` is drained PER FRAME: each frame reports
/// `busy: calls = 1`, so "a functions entry named busy with calls >= 30" can
/// only mean across the >= 30 frames the acceptance asks for.
fn total_calls(profiler: &serde_json::Value, name: &str) -> u64 {
    profiler["frames"]
        .as_array()
        .map(|frames| {
            frames
                .iter()
                .filter_map(|f| f["functions"].as_array())
                .flatten()
                .filter(|e| e["name"] == name)
                .filter_map(|e| e["calls"].as_u64())
                .sum()
        })
        .unwrap_or(0)
}

/// **The acceptance test.**
#[test]
fn profiler_records_frames_from_play_loop() {
    let (handle, port, _dir) = server();

    // -- nothing recorded before Play ---------------------------------------
    let idle = json_of(&http_get(port, "/api/profiler"));
    assert_eq!(
        idle["count"], 0,
        "no frames can have been captured before the game runs: {idle}"
    );

    // -- Start the profiler, then Play --------------------------------------
    let started = json_of(&http_post(port, "/api/profiler/start", "{}"));
    assert_eq!(started["capturing"], true, "start failed: {started}");

    let played = json_of(&http_post(port, "/api/runtime/play", "{}"));
    assert_eq!(played["ok"], true, "play failed: {played}");

    // -- wait for 30 frames --------------------------------------------------
    //
    // Polled rather than slept-for: the driver's cadence is the project's
    // `delta_time`, so a fixed sleep would either be flaky on a slow machine or
    // waste a second on a fast one.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut profiler = json_of(&http_get(port, "/api/profiler"));
    while profiler["count"].as_u64().unwrap_or(0) < 30 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
        profiler = json_of(&http_get(port, "/api/profiler"));
    }
    let stopped = json_of(&http_post(port, "/api/profiler/stop", "{}"));
    assert_eq!(stopped["capturing"], false, "stop failed: {stopped}");

    let count = profiler["count"].as_u64().unwrap_or(0);
    assert!(
        count >= 30,
        "the play loop must record a frame per tick; got {count} after 10s. \
         Zero here is the old behaviour — nothing in the tick recorded, and \
         the only producer was a route no client calls: {profiler}"
    );

    // -- every captured frame carries a real measurement ---------------------
    let frames = profiler["frames"].as_array().expect("frames array");
    let measured = frames
        .iter()
        .filter(|f| f["total_ms"].as_f64().unwrap_or(0.0) > 0.0)
        .count();
    assert_eq!(
        measured,
        frames.len(),
        "every recorded frame must carry a measured total_ms > 0; {} of {} did \
         not. A zero means the frame was pushed without being timed",
        frames.len() - measured,
        frames.len()
    );

    // -- the per-function rows come from the interpreter ---------------------
    let busy_calls = total_calls(&profiler, "busy");
    assert!(
        busy_calls >= 30,
        "`busy()` is called once per `_process`, so >= 30 frames must account \
         for >= 30 calls; got {busy_calls}. Zero means `ProfilerFrame.functions` \
         was never fed — the interpreter hook is what fills it: {profiler}"
    );
    let process_calls = total_calls(&profiler, "_process");
    assert!(
        process_calls >= 30,
        "...and `_process` itself is a row too, so the Self column can \
         distinguish the caller from the callee; got {process_calls}"
    );

    // Self time must NOT be a copy of inclusive time: `_process` spends almost
    // all of its time inside `busy()`, so its self time has to be the smaller
    // of the two. This is what the Self column exists for, and the one thing a
    // naive implementation gets wrong by assigning `self_ms = time_ms`.
    let process_row = frames
        .iter()
        .filter_map(|f| f["functions"].as_array())
        .flatten()
        .find(|e| e["name"] == "_process")
        .unwrap_or_else(|| panic!("no `_process` row in any frame: {profiler}"));
    let incl = process_row["time_ms"].as_f64().unwrap_or(0.0);
    let own = process_row["self_ms"].as_f64().unwrap_or(0.0);
    assert!(
        own < incl,
        "`_process` calls `busy()`, so its SELF time must be less than its \
         inclusive time; got self={own} inclusive={incl}. Equal means self time \
         is just a copy of inclusive time, which makes the column useless"
    );

    // -- Video RAM lists what is actually resident ---------------------------
    let info = json_of(&http_get(port, "/api/debug/info"));
    let vram = info["video_ram"].as_array().expect("video_ram array");
    let names: Vec<&str> = vram.iter().filter_map(|r| r["resource"].as_str()).collect();
    assert!(
        names.contains(&"res://icon.png"),
        "Video RAM must list the texture the project actually loaded; got \
         {names:?}"
    );
    assert!(
        !names.contains(&"icon.svg") && !names.contains(&"font_default"),
        "...and none of the hard-coded rows this replaced; got {names:?}"
    );
    let icon_bytes = vram
        .iter()
        .find(|r| r["resource"] == "res://icon.png")
        .and_then(|r| r["usage_bytes"].as_u64())
        .unwrap_or(0);
    assert_eq!(
        icon_bytes, TEX_BYTES,
        "the size must be DERIVED from the texture ({TEX_W}x{TEX_H} RGBA8 = \
         {TEX_BYTES} bytes), not a literal — a round number like 262144 for a \
         48x32 image is the tell that nothing measured it"
    );
    assert!(
        info["total_vram_bytes"].as_u64().unwrap_or(0) >= TEX_BYTES,
        "the total must account for the rows beneath it: {info}"
    );

    drop(handle);
}
