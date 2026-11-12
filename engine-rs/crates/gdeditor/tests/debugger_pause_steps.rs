//! pat-mgixk.14 acceptance — a breakpoint actually pauses the running game, and
//! Step Over / Continue / Evaluate do what they say.
//!
//! Before this, every piece of the debugger existed and none of it was
//! connected: `gdcore::debugger::Debugger::should_break` had no caller outside
//! its own unit tests, `api_debug_step_in`/`step_over`/`step_out` each only
//! flipped `debug_state` from "paused" to "running" (so all three were
//! indistinguishable from Continue), `api_debug_break` PUSHED A FABRICATED
//! FRAME — `("_process", "res://main.gd", 1)` — and `debug_locals` was
//! declared, initialised and cleared but never written, so `/api/debug/locals`
//! and the expression evaluator always came back empty.
//!
//! # Why the pause is a suspension and not a blocked thread
//!
//! The obvious implementation blocks the thread running the script until the
//! editor says step. That deadlocks this server: `run_runtime_driver` calls
//! `advance_running_main_loop` **while holding the `EditorState` mutex**, and
//! scripts run inside that frame — so a blocked script holds the one lock every
//! route needs, including the step routes that would release it. The pause
//! therefore goes through the interpreter's existing statement-level
//! suspend/resume machine (the one `await` uses): the frame is PARKED with its
//! remaining statements and its locals, and a step resumes it from exactly
//! there. See `gdscript_interop::debug_session`.
//!
//! # Why this test drives the real HTTP routes
//!
//! The hazard is threading, and only an end-to-end run exposes it. The pause is
//! published by the thread that runs scripts (`build_play_tree`, inline on the
//! connection thread serving `POST /api/runtime/play`), while the Step command
//! arrives on a *different* connection thread, and the interpreter's debugger
//! is a `thread_local!`. An implementation that kept the pause in that
//! thread-local would compile, pass a unit test, and report "not paused" to
//! every route.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// The acceptance's script, with the line numbers it is asserted against:
///
/// ```text
/// 1  extends Node2D
/// 2  var marker = 0
/// 3  func _ready():
/// 4      var x = 41
/// 5      x = x + 1
/// 6      self.marker = x
/// ```
///
/// The breakpoint goes on **line 5**, the `x = x + 1`. That is the line whose
/// statement has NOT run when the debugger stops, which is what makes `x` read
/// 41 at the pause and 42 after one Step Over — the two different values the
/// acceptance distinguishes. A breakpoint on line 4 would read `x` as absent,
/// and one on line 6 as 42 before any step: either would make the step
/// unobservable.
///
/// `x = x + 1` rather than `x += 1` for parser-independence, matching the
/// sibling scripted tests.
const MAIN_GD: &str = "\
extends Node2D
var marker = 0
func _ready():
    var x = 41
    x = x + 1
    self.marker = x
";

/// The line `MAIN_GD` is broken on.
const BREAK_LINE: u64 = 5;

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

fn body_of(resp: &str) -> &str {
    resp.split("\r\n\r\n").nth(1).unwrap_or("")
}

fn json_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(body_of(resp))
        .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {resp}"))
}

/// The value of a local the debugger reports, or `None` when it is absent.
fn local(state: &serde_json::Value, name: &str) -> Option<String> {
    state["locals"]
        .as_array()?
        .iter()
        .find(|v| v["name"] == name)
        .and_then(|v| v["value"].as_str())
        .map(str::to_string)
}

/// A one-node scene whose script is a real `.gd` file on disk.
///
/// ⚠️ BOTH roots must be set. `attach_scripts_to_tree` resolves `_script_path`
/// against `texture_cache.project_root()`, not `state.project_root`; setting
/// only the latter attaches no script at all, and every assertion here would
/// fail as "never paused" rather than "the script never loaded".
fn server() -> (EditorServerHandle, u16, tempfile::TempDir) {
    gdobject::class_db::register_2d_classes();

    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.gd"), MAIN_GD).expect("write script");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("res://main.gd".to_string()));
    tree.add_child(root, main).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    (handle, port, dir)
}

/// **The acceptance test.**
#[test]
fn debugger_breakpoint_pauses_play_and_steps() {
    let (handle, port, _dir) = server();

    // -- set the breakpoint from the script-editor gutter --------------------
    //
    // Through the gutter route on purpose: the bead's last item is that a
    // breakpoint toggled there must register in the SAME debugger the
    // interpreter consults. There were two editor-side lists and neither was
    // the runtime's.
    let toggled = json_of(&http_post(
        port,
        "/api/script/breakpoint/toggle",
        &format!(r#"{{"path":"res://main.gd","line":{BREAK_LINE}}}"#),
    ));
    assert_eq!(
        toggled["added"], true,
        "toggling must report the breakpoint as now SET: {toggled}"
    );

    // -- Play: `_ready` runs inline here and must stop on line 5 -------------
    let played = json_of(&http_post(port, "/api/runtime/play", "{}"));
    assert_eq!(played["ok"], true, "play failed: {played}");

    let paused = json_of(&http_get(port, "/api/debug/state"));
    assert_eq!(
        paused["state"], "paused",
        "a breakpoint must pause the running game; `running` here means the \
         interpreter never consulted the debugger: {paused}"
    );

    // -- a REAL stack, not the fabricated one --------------------------------
    let frames = paused["frames"]
        .as_array()
        .unwrap_or_else(|| panic!("debug state must carry a frames array: {paused}"));
    assert!(
        !frames.is_empty(),
        "a pause must report the stack it stopped on: {paused}"
    );
    assert_eq!(
        frames[0]["function"], "_ready",
        "the innermost frame is the function that broke — the old code pushed \
         a hard-coded `_process` here: {paused}"
    );
    assert_eq!(
        frames[0]["script"], "res://main.gd",
        "and the script it belongs to, which used to be a literal: {paused}"
    );
    assert_eq!(
        frames[0]["line"], BREAK_LINE,
        "stopped on the breakpointed line: {paused}"
    );

    // -- locals: `x` is 41, because line 5 has NOT run yet -------------------
    let locals = json_of(&http_get(port, "/api/debug/locals"));
    assert_eq!(
        local(&locals, "x").as_deref(),
        Some("41"),
        "`/api/debug/locals` must carry the paused frame's locals, and `x` must \
         still be 41 because the statement on line {BREAK_LINE} has not run. \
         An empty list here is the old behaviour (nothing ever wrote \
         `debug_locals`); a 42 means the debugger stopped AFTER the statement \
         instead of before it: {locals}"
    );

    // -- Step Over: one statement runs, and it stops on the next line --------
    let stepped = json_of(&http_post(port, "/api/debug/step_over", "{}"));
    assert_eq!(stepped["ok"], true, "step_over failed: {stepped}");
    assert_eq!(
        stepped["paused"], true,
        "Step Over must stop again on the NEXT statement. `false` here means \
         the frame resumed and ran to the end — which is Continue, and is what \
         all three Step buttons used to do: {stepped}"
    );
    assert_eq!(
        stepped["line"],
        BREAK_LINE + 1,
        "...and the next statement is line {}: {stepped}",
        BREAK_LINE + 1
    );

    let after_step = json_of(&http_get(port, "/api/debug/locals"));
    assert_eq!(
        local(&after_step, "x").as_deref(),
        Some("42"),
        "the stepped statement must actually have RUN: `x` was 41 and \
         `x = x + 1` makes it 42. Still 41 means the step reported progress \
         without executing anything: {after_step}"
    );

    // -- Evaluate against the paused frame -----------------------------------
    let evaluated = json_of(&http_post(
        port,
        "/api/debug/evaluate",
        r#"{"expression":"x * 2"}"#,
    ));
    assert_eq!(
        evaluated["result"], "84",
        "the evaluator resolves names against the PAUSED frame; it always \
         answered 0 before, because the locals it looks `x` up in were never \
         filled: {evaluated}"
    );

    // -- Continue: the frame finishes and frames resume ----------------------
    let resumed = json_of(&http_post(port, "/api/debug/continue", "{}"));
    assert_eq!(resumed["ok"], true, "continue failed: {resumed}");
    assert_eq!(
        resumed["paused"], false,
        "Continue must run the rest of the frame, not stop on the next line: \
         {resumed}"
    );

    let running = json_of(&http_get(port, "/api/debug/state"));
    assert_ne!(
        running["state"], "paused",
        "the session must be running again after Continue: {running}"
    );
    assert!(
        running["frames"]
            .as_array()
            .map(|f| f.is_empty())
            .unwrap_or(false),
        "a resumed game must not keep showing the stack it was paused on — a \
         panel still listing frames is how a running game reads as paused \
         forever: {running}"
    );

    // ...and the game really is advancing again, which is the other half of
    // "Continue resumes frames": the driver thread stops stepping while a
    // pause is live, so a frame count that grows proves it was released.
    let before = json_of(&http_get(port, "/api/runtime/status"));
    let first = before["frame_count"].as_u64().unwrap_or(0);
    thread::sleep(Duration::from_millis(400));
    let later = json_of(&http_get(port, "/api/runtime/status"));
    assert!(
        later["frame_count"].as_u64().unwrap_or(0) > first,
        "the runtime driver must step frames again once the pause is gone; \
         went {first} -> {:?}",
        later["frame_count"]
    );

    drop(handle);
}
