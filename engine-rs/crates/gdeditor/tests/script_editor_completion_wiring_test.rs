//! pat-0q2kx.14 acceptance: the served script editor's completion popup and
//! Ctrl+Click goto-definition are wired to live endpoints.
//!
//! The server half was already green — `POST /api/script/complete` had a unit
//! test, and `goto_definition.rs` had its own tests — but nothing in
//! `EDITOR_HTML` called either, so the editor had no popup and Ctrl+Click did
//! nothing. `goto_definition.rs` had no caller at all outside its own tests.
//!
//! Three layers are covered, because any one alone can pass while the feature
//! is dead:
//!  1. **Static** — the served HTML actually contains the popup and fetches the
//!     endpoints (catches "server works, client never calls it", the exact
//!     shape of this bug).
//!  2. **Endpoint** — completion returns a known local; goto returns file+line
//!     for a method, including one defined in a *different* project script.
//!  3. **End-to-end** — a buffer with a completed identifier saved through
//!     `/api/script/save` lands on disk with that identifier.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

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
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"),
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

fn temp_project(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx14_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"ScriptCompletionE2E\"\n",
    )
    .unwrap();
    dir
}

fn server_on_temp_project(tag: &str) -> (EditorServerHandle, u16, PathBuf) {
    let dir = temp_project(tag);
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    let handle = EditorServerHandle::start(port, EditorState::new(tree));
    thread::sleep(Duration::from_millis(100));
    let open = http_post(
        port,
        "/api/project/open",
        &serde_json::json!({ "path": dir.display().to_string() }).to_string(),
    );
    assert!(open.contains("200 OK"), "/api/project/open failed: {open}");
    (handle, port, dir)
}

// ---------------------------------------------------------------------------
// (1) Static wiring — the client must actually call the endpoints
// ---------------------------------------------------------------------------

#[test]
fn editor_html_wires_the_completion_popup_and_goto() {
    assert!(
        EDITOR_HTML.contains("script-completion-popup"),
        "EDITOR_HTML must contain the completion popup element"
    );
    assert!(
        EDITOR_HTML.contains("/api/script/complete"),
        "EDITOR_HTML must POST /api/script/complete as you type"
    );
    assert!(
        EDITOR_HTML.contains("/api/script/goto"),
        "EDITOR_HTML must GET /api/script/goto for Ctrl+Click goto-definition"
    );
    // The popup is only useful if a key accepts the selection and a request is
    // actually issued — guard the wiring functions, not just the strings.
    assert!(
        EDITOR_HTML.contains("scriptCompletionAccept"),
        "EDITOR_HTML must wire an accept path that inserts the candidate"
    );
    assert!(
        EDITOR_HTML.contains("scriptCompletionHandleKey"),
        "EDITOR_HTML must route Tab/Enter/arrows to the popup while it is open"
    );
    assert!(
        EDITOR_HTML.contains("scriptGotoDefinitionAtCaret"),
        "EDITOR_HTML must wire Ctrl+Click to the goto request"
    );
}

// ---------------------------------------------------------------------------
// (2) Endpoint round-trips
// ---------------------------------------------------------------------------

/// Completion surfaces a local declared in the buffer.
#[test]
fn script_complete_returns_a_known_local() {
    let (_handle, port, _dir) = server_on_temp_project("complete");

    let resp = http_post(
        port,
        "/api/script/complete",
        r#"{"current_class":"Node2D","prefix":"vel","locals":["velocity_scale","health","speed"]}"#,
    );
    assert!(resp.contains("200 OK"), "completion must answer: {resp}");

    let v = json_of(&resp);
    let labels: Vec<String> = v["candidates"]
        .as_array()
        .expect("candidates array")
        .iter()
        .filter_map(|c| c["label"].as_str().map(String::from))
        .collect();
    assert!(
        labels.iter().any(|l| l == "velocity_scale"),
        "the local matching the prefix must be offered, got {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l == "health"),
        "a local that does not match the prefix must not be offered, got {labels:?}"
    );
}

/// Goto resolves a method defined in the open script.
#[test]
fn script_goto_returns_file_and_line_for_a_method() {
    let (_handle, port, dir) = server_on_temp_project("goto");
    let script = "extends Node\n\nvar speed = 10\n\nfunc _ready():\n    take_damage(1)\n\nfunc take_damage(amount):\n    speed -= amount\n";
    std::fs::write(dir.join("player.gd"), script).unwrap();

    let resp = http_get(
        port,
        "/api/script/goto?path=res://player.gd&symbol=take_damage",
    );
    assert!(resp.contains("200 OK"), "goto must answer: {resp}");
    let v = json_of(&resp);
    assert_eq!(v["found"], serde_json::json!(true), "symbol resolved: {v}");
    assert_eq!(
        v["line"],
        serde_json::json!(8),
        "`func take_damage` is on line 8 (1-based): {v}"
    );
    assert_eq!(v["file"], serde_json::json!("res://player.gd"));

    // The caret form resolves the identifier under the column, like Ctrl+Click.
    let caret = http_get(
        port,
        "/api/script/goto?path=res://player.gd&line=%20%20%20%20take_damage(1)&col=6",
    );
    let cv = json_of(&caret);
    assert_eq!(
        cv["line"],
        serde_json::json!(8),
        "the identifier under the caret resolves to the same definition: {cv}"
    );

    // An unresolved symbol is a no-op, not an error — Godot does nothing.
    let missing = http_get(port, "/api/script/goto?path=res://player.gd&symbol=nope");
    assert!(missing.contains("200 OK"), "unresolved must not error");
    assert_eq!(json_of(&missing)["found"], serde_json::json!(false));
}

/// Goto falls back to other project scripts, which is why the reply names a
/// `file` at all.
#[test]
fn script_goto_finds_a_definition_in_another_project_script() {
    let (_handle, port, dir) = server_on_temp_project("gotocross");
    std::fs::write(
        dir.join("player.gd"),
        "extends Node\n\nfunc _ready():\n    Utils.helper()\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(
        dir.join("lib").join("utils.gd"),
        "extends Node\n\nfunc helper():\n    pass\n",
    )
    .unwrap();

    let resp = http_get(port, "/api/script/goto?path=res://player.gd&symbol=helper");
    let v = json_of(&resp);
    assert_eq!(
        v["found"],
        serde_json::json!(true),
        "a symbol defined in another script still resolves: {v}"
    );
    assert_eq!(v["file"], serde_json::json!("res://lib/utils.gd"));
    assert_eq!(v["line"], serde_json::json!(3));
}

// ---------------------------------------------------------------------------
// (3) End-to-end: the completed identifier reaches disk
// ---------------------------------------------------------------------------

/// **The acceptance test.** Complete a prefix, apply the winning candidate to
/// the buffer the way the popup does, save through `/api/script/save`, and read
/// the file back — the completed identifier is on disk.
#[test]
fn script_editor_completion_wiring_e2e() {
    let (_handle, port, dir) = server_on_temp_project("e2e");
    let script_path = dir.join("player.gd");

    // A buffer mid-edit: the author has typed `vel` and wants the local.
    let before = "extends Node2D\n\nvar velocity_scale = 2.0\n\nfunc _ready():\n    var v = vel\n";
    std::fs::write(&script_path, before).unwrap();

    // 1. Ask the server what completes `vel` — exactly what the popup posts.
    let resp = http_post(
        port,
        "/api/script/complete",
        r#"{"current_class":"Node2D","prefix":"vel","locals":["velocity_scale","v"]}"#,
    );
    let v = json_of(&resp);
    let chosen = v["candidates"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|c| c["label"].as_str())
        .expect("at least one candidate for `vel`")
        .to_string();
    assert_eq!(
        chosen, "velocity_scale",
        "the local should rank first for prefix `vel`"
    );

    // 2. Apply it the way `scriptCompletionAccept` does: replace the typed
    //    prefix with the candidate label.
    let after = before.replace("var v = vel", &format!("var v = {chosen}"));
    assert!(after.contains("var v = velocity_scale"));

    // 3. Save the edited buffer through the real route.
    let saved = http_post(
        port,
        "/api/script/save",
        &serde_json::json!({
            "path": script_path.display().to_string(),
            "content": after,
        })
        .to_string(),
    );
    assert!(saved.contains("200 OK"), "save must succeed: {saved}");

    // 4. The completed identifier is on disk — not just in a popup.
    let on_disk = std::fs::read_to_string(&script_path).expect("script readable");
    assert!(
        on_disk.contains("var v = velocity_scale"),
        "the completed identifier must be persisted by /api/script/save, got:\n{on_disk}"
    );

    // 5. And it is now itself resolvable by goto — the loop closes.
    let goto = json_of(&http_get(
        port,
        "/api/script/goto?path=res://player.gd&symbol=velocity_scale",
    ));
    assert_eq!(goto["found"], serde_json::json!(true));
    assert_eq!(
        goto["line"],
        serde_json::json!(3),
        "`var velocity_scale` is on line 3: {goto}"
    );
}
