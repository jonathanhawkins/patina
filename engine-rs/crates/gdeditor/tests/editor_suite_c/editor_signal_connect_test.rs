//! pat-mgixk.13 acceptance: a signal connected in the served editor is a REAL
//! connection — it persists as a `[connection]` line, it fires in Play, and the
//! Node dock lists it again on reload.
//!
//! # What was wrong
//!
//! `api_connect_signal` appended `"signal:method"` to a `signal_connections`
//! string property and dropped `target_id`, `deferred`, `one_shot` and `binds`
//! on the floor. Its own comment said it avoided `SceneTree::connect_signal`
//! deliberately. Nothing at runtime read that property, so **a connection made
//! in the editor never fired**; `TscnSaver` writes `[connection]` only from the
//! tree's signal store, so it was **never saved** either; and because
//! `is_internal_prop` did not know the key, the property itself leaked into the
//! `.tscn` as a stray field Godot has never heard of.
//!
//! Disconnect was worse: connect joined entries with `,` while disconnect split
//! on `;`, so the whole value was always a single entry, and the filter was
//! `!entry.contains(&signal)` — a substring test. Between the two, removing one
//! connection dropped every connection on the node, with no undo.
//!
//! # Why this enters through HTTP
//!
//! The bead carries a `Wiring requirement`, so the test drives the real editor
//! server over TCP, exactly as the browser does. Nothing here calls
//! `EditorCommand` or `SceneTree::connect_signal`; if the route stops reaching
//! them, these tests fail.
//!
//! ```text
//! POST /api/node/signals/connect   (the Connect dialog's payload)
//!   -> EditorCommand::ConnectSignal -> SceneTree signal store
//!     -> POST /api/scene/save      -> [connection ...] in the .tscn
//!     -> GET  /api/node/signals    -> the Node dock's list
//!     -> POST /api/runtime/play    -> a real Button click runs the method
//! ```

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// The receiver. `_on_pressed` is what the connection names; `hits` is the
/// observable a click has to move.
///
/// `_on_other` exists so the "disconnect one of two" case has two connections
/// on the SAME signal that differ only by method — which is the case the old
/// substring filter destroyed wholesale.
const MAIN_GD: &str = "\
extends Node2D
var hits = 0
var others = 0

func _on_pressed():
    self.hits += 1

func _on_other():
    self.others += 1
";

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    stream.write_all(request.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).to_string()
}

fn http_get(port: u16, path: &str) -> String {
    http(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// Parses the JSON body out of a raw HTTP response.
fn json_body(response: &str) -> serde_json::Value {
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or(response);
    serde_json::from_str(body.trim()).unwrap_or_else(|e| {
        panic!("response body is not JSON ({e}): {response}");
    })
}

/// Boots the real editor server over `/root -> Main(Node2D, scripted) -> Button`.
///
/// ⚠️ BOTH project roots are required and setting only one is silent: the Play
/// path resolves `_script_path` against `texture_cache.project_root()`, and
/// without it no script attaches — which surfaces as every script variable
/// being *absent* rather than as "the script did not load".
fn boot() -> (EditorServerHandle, u16, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("main.gd"), MAIN_GD).expect("write script");

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut main = Node::new("Main", "Node2D");
    main.set_property("_script_path", Variant::String("main.gd".to_string()));
    let main_id = tree.add_child(root, main).unwrap();

    // A real Button, so the Play half can click it rather than emitting the
    // signal by hand.
    let mut button = Node::new("Button", "Button");
    button.set_property("position", Variant::Vector2(gdcore::math::Vector2::ZERO));
    button.set_property(
        "size",
        Variant::Vector2(gdcore::math::Vector2::new(80.0, 40.0)),
    );
    tree.add_child(main_id, button).unwrap();

    let mut state = EditorState::new(tree);
    let root_path = dir.path().to_string_lossy().to_string();
    state.project_root = Some(root_path.clone());
    state.texture_cache.set_project_root(root_path);

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(150));
    (handle, port, dir)
}

/// The editor-side id of a node, by name, out of the scene tree endpoint.
///
/// ⚠️ `GET /api/scene` answers `{"nodes": <tree>, "recommended_view": ...}` —
/// the tree is UNDER `nodes`, not at the top level. Walking the response object
/// directly finds no `name` and no `children`, so it returns `None` for every
/// node and this helper panics on the first lookup.
fn node_id(port: u16, name: &str) -> u64 {
    let response = json_body(&http_get(port, "/api/scene"));
    let scene = response
        .get("nodes")
        .cloned()
        .unwrap_or_else(|| panic!("/api/scene has no `nodes` tree: {response}"));
    fn walk(node: &serde_json::Value, name: &str) -> Option<u64> {
        if node.get("name").and_then(|n| n.as_str()) == Some(name) {
            return node.get("id").and_then(|i| i.as_u64());
        }
        for child in node.get("children")?.as_array()? {
            if let Some(found) = walk(child, name) {
                return Some(found);
            }
        }
        None
    }
    walk(&scene, name).unwrap_or_else(|| panic!("no node named `{name}` in {scene}"))
}

/// The dock's connection list for one signal on one node.
fn dock_connections(port: u16, node: u64, signal: &str) -> Vec<serde_json::Value> {
    let dock = json_body(&http_get(
        port,
        &format!("/api/node/signals?node_id={node}"),
    ));
    dock.get("signals")
        .and_then(|s| s.as_array())
        .and_then(|signals| {
            signals
                .iter()
                .find(|s| s.get("name").and_then(|n| n.as_str()) == Some(signal))
        })
        .and_then(|s| s.get("connections"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default()
}

fn connect(
    port: u16,
    node: u64,
    target: u64,
    signal: &str,
    method: &str,
    deferred: bool,
) -> String {
    http_post(
        port,
        "/api/node/signals/connect",
        &format!(
            r#"{{"node_id":{node},"target_id":{target},"signal":"{signal}",
                 "method":"{method}","deferred":{deferred},"one_shot":false,"binds":[]}}"#
        ),
    )
}

/// **The acceptance test.** Connect in the editor, save, reload, disconnect one
/// of two, undo — and click the button in Play.
#[test]
fn editor_signal_connect_persists_and_fires_in_play() {
    let (handle, port, dir) = boot();
    let button = node_id(port, "Button");
    let main = node_id(port, "Main");

    // -- (1) the connection is real, not a property --------------------------
    assert!(
        connect(port, button, main, "pressed", "_on_pressed", true).contains("200 OK"),
        "the connect route must accept the Connect dialog's payload"
    );

    let saved = dir.path().join("scene.tscn");
    let save = http_post(
        port,
        "/api/scene/save",
        &format!(r#"{{"path":"{}"}}"#, saved.to_string_lossy()),
    );
    assert!(
        save.contains("200 OK"),
        "saving the scene must succeed: {save}"
    );
    let tscn = std::fs::read_to_string(&saved).expect("the saved scene exists");

    // `flags=1` is CONNECT_DEFERRED. ⚠️ Godot's ConnectFlags are NOT
    // consecutive — DEFERRED=1, PERSIST=2, ONE_SHOT=4 — so a `flags=2` here
    // would mean CONNECT_PERSIST and reload as a different connection.
    assert!(
        tscn.contains(
            r#"[connection signal="pressed" from="Button" to="." method="_on_pressed" flags=1]"#
        ),
        "the editor's connection must be saved as a real [connection] with its \
         Deferred flag. Got:\n{tscn}"
    );
    assert!(
        !tscn.contains("signal_connections"),
        "the retired `signal_connections` property must not leak into the \
         scene — Godot has no such field. Got:\n{tscn}"
    );

    // -- (2) the Node dock lists it from the store ---------------------------
    let listed = dock_connections(port, button, "pressed");
    assert_eq!(
        listed.len(),
        1,
        "the dock must list the connection: {listed:?}"
    );
    assert_eq!(listed[0]["method"], "_on_pressed");
    assert_eq!(
        listed[0]["target_id"].as_u64(),
        Some(main),
        "the connection must name the node it actually targets, not the source"
    );
    assert_eq!(listed[0]["flags"], 1, "Deferred must survive into the dock");

    // `pressed` is a ClassDB signal of Button, not one of the four classes the
    // old hard-coded table knew. Listing it at all proves the inheritance walk
    // replaced that table.
    let dock = json_body(&http_get(
        port,
        &format!("/api/node/signals?node_id={button}"),
    ));
    let names: Vec<&str> = dock["signals"]
        .as_array()
        .expect("signals array")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(
        names.contains(&"pressed"),
        "Button's ClassDB signals must reach the dock: {names:?}"
    );

    // -- (3) disconnecting one of two leaves the other -----------------------
    assert!(
        connect(port, button, main, "pressed", "_on_other", false).contains("200 OK"),
        "a second connection on the same signal must be allowed"
    );
    assert_eq!(
        dock_connections(port, button, "pressed").len(),
        2,
        "both connections should be live before the disconnect"
    );

    let disconnect = http_post(
        port,
        "/api/signal/disconnect",
        &format!(
            r#"{{"node_id":{button},"signal":"pressed","method":"_on_other","target_id":{main}}}"#
        ),
    );
    assert!(disconnect.contains("200 OK"), "disconnect must succeed");

    let remaining = dock_connections(port, button, "pressed");
    assert_eq!(
        remaining.len(),
        1,
        "disconnecting ONE connection must leave the other. The old handler \
         split on the wrong separator and matched by substring, so it removed \
         every connection on the node: {remaining:?}"
    );
    assert_eq!(
        remaining[0]["method"], "_on_pressed",
        "the surviving connection must be the one that was not named"
    );

    // -- (4) undo brings it back, with its flags -----------------------------
    assert!(
        http_post(port, "/api/undo", "{}").contains("200 OK"),
        "the disconnect must be undoable — the old handler pushed nothing"
    );
    let restored = dock_connections(port, button, "pressed");
    assert_eq!(restored.len(), 2, "undo must restore the connection");
    let other = restored
        .iter()
        .find(|c| c["method"] == "_on_other")
        .expect("the undone connection is back");
    assert_eq!(
        other["deferred"], false,
        "undo must restore the flags the connection HAD, not defaults"
    );

    // -- (5) it fires in Play ------------------------------------------------
    //
    // ⚠️ `/api/runtime/step` 400s on an unpaused runtime: an unpaused one is
    // advanced by a background driver instead. Pausing is what makes stepping
    // legal and these assertions deterministic.
    assert!(
        http_post(port, "/api/runtime/play", "{}").contains("200 OK"),
        "the runtime must start"
    );
    http_post(port, "/api/runtime/pause", "");

    let before = script_int(port, "hits");
    assert_eq!(
        before, 0,
        "the counter starts at zero, or the click below proves nothing"
    );

    // A real click: the press has to land ON the button for the control to take
    // it, and `pressed` fires on release.
    //
    // ⚠️ `button` is the **DOM** `MouseEvent.button` index, so LEFT is **0**.
    // `api_input_mouse_down` reads only `v["button"]` (rejecting anything above
    // 4) and ignores `x`/`y` entirely — the cursor position comes from the
    // `mouse_move` above. A `1` here is a MIDDLE click: the press is accepted
    // with 200 OK and the handler simply never runs, which is why this failed on
    // the counter rather than on the request.
    // ⚠️ ONE input event per frame, each followed by a step — the cadence
    // `wheel_scrolls_a_scroll_container` uses (move, step, event, step). Input
    // is a per-frame snapshot that `step` consumes, so firing move+down+up into
    // a single frame gives a BaseButton no press-then-release transition to
    // observe, and `pressed` never fires. Batching them is what left the
    // counter at 0 even with the correct button index.
    http_post(
        port,
        "/api/runtime/input/mouse_move",
        r#"{"x":10.0,"y":10.0}"#,
    );
    http_post(port, "/api/runtime/step", "");
    http_post(port, "/api/runtime/input/mouse_down", r#"{"button":0}"#);
    http_post(port, "/api/runtime/step", "");
    http_post(port, "/api/runtime/input/mouse_up", r#"{"button":0}"#);
    http_post(port, "/api/runtime/step", "");
    // One more: the connection is DEFERRED, so the callback runs from the
    // deferred queue on the frame AFTER the emission rather than inline.
    http_post(port, "/api/runtime/step", "");

    assert_eq!(
        script_int(port, "hits"),
        1,
        "clicking the Button in Play must run the connected method. Before this \
         bead the connection lived in a string property nothing read, so this \
         counter never moved however correct the editor looked."
    );

    http_post(port, "/api/runtime/stop", "");
    handle.stop();
}

/// Reads a script variable off the RUNNING scene.
///
/// ⚠️ The variable name is a **value**, not a key, and `value` is doubled.
/// Walking the JSON for a key named `hits` matches nothing, ever.
fn script_int(port: u16, name: &str) -> i64 {
    let node = json_body(&http_get(port, "/api/runtime/node?path=/root/Main"));
    node.get("properties")
        .and_then(|p| p.as_array())
        .and_then(|props| {
            props
                .iter()
                .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
        })
        .and_then(|p| p.get("value"))
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| panic!("script var `{name}` missing from /api/runtime/node: {node}"))
}

/// The connect route must honour `target_id`. Defaulting every connection to
/// the source node is what made `to=` meaningless in a saved scene.
#[test]
fn connect_honours_the_requested_target() {
    let (handle, port, _dir) = boot();
    let button = node_id(port, "Button");
    let main = node_id(port, "Main");

    connect(port, button, main, "pressed", "_on_pressed", false);
    let listed = dock_connections(port, button, "pressed");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0]["target_id"].as_u64(),
        Some(main),
        "the target must be the node the request named"
    );
    assert_eq!(
        listed[0]["target_path"].as_str(),
        Some("/root/Main"),
        "and it must resolve back to a real node path, or the saver cannot \
         write a `to=` for it"
    );
    // `Go to Method` needs the RECEIVER's script, not the emitter's. The two
    // differ for every connection worth making, and the menu item was a toast
    // until this field existed.
    assert_eq!(
        listed[0]["target_script"].as_str(),
        Some("main.gd"),
        "the connection must carry the target's script so Go to Method can \
         open it"
    );

    handle.stop();
}

/// Omitting `target_id` must default to the SCENE root — the first child of
/// `/root`, which is what `to="."` means — not to `/root` itself.
///
/// `/root` is the editor's own Window and is not part of the saved subtree, so
/// a connection targeting it resolves to `"."` only through the saver's
/// fallback: the right text by accident, and a connection that names nothing
/// real on reload.
#[test]
fn connect_defaults_the_target_to_the_scene_root() {
    let (handle, port, _dir) = boot();
    let button = node_id(port, "Button");
    let main = node_id(port, "Main");

    http_post(
        port,
        "/api/node/signals/connect",
        &format!(r#"{{"node_id":{button},"signal":"pressed","method":"_on_pressed"}}"#),
    );
    let listed = dock_connections(port, button, "pressed");
    assert_eq!(listed.len(), 1, "the connection must be made: {listed:?}");
    assert_eq!(
        listed[0]["target_id"].as_u64(),
        Some(main),
        "the default target must be the scene root, not /root"
    );

    handle.stop();
}

/// The Connect dialog asks the server to CREATE the receiver stub when the
/// method does not exist yet, as Godot's `ConnectDialog` does.
///
/// ⚠️ A served-string assertion, which is weaker than the HTTP tests above and
/// is used deliberately: the gap was client-side only. `/api/signal/autocreate_receiver`
/// has existed since pat-jwuv1.4, has a handler and a unit test
/// (`signal_receiver_stub.rs`), and had **no caller in the served editor at
/// all** — so the dialog connected a signal to a method nobody had written.
/// Driving real browser JS is out of reach here, so this pins the call SHAPE
/// rather than just the route literal: a bare `contains` on the path would pass
/// on a mention in a comment.
#[test]
fn connect_dialog_requests_a_receiver_stub() {
    // 🔑 The needle is the CALL SYNTAX, not the route path. Both weaker needles
    // were checked against the served string and both are false positives:
    //
    //   * `contains("/api/signal/autocreate_receiver")` also matches this
    //     bead's own explanatory comment in the dialog handler;
    //   * `contains("path: conn.target_script")` also matches the pre-existing
    //     "Go to Method" call, which sends the same field — so it would stay
    //     green with this wiring deleted.
    //
    // `'<route>', {` can only be the `api('POST', ...)` call, and it occurs
    // exactly once.
    const CALL: &str = "'/api/signal/autocreate_receiver', {";
    assert_eq!(
        EDITOR_HTML.matches(CALL).count(),
        1,
        "the served editor must call the autocreate route exactly once; it had \
         NO client caller at all before pat-mgixk.13, so the dialog connected a \
         signal to a method nobody had written",
    );

    let at = EDITOR_HTML.find(CALL).expect("checked by the count above");
    let call_site = &EDITOR_HTML[at..(at + 300).min(EDITOR_HTML.len())];
    assert!(
        call_site.contains("path: conn.target_script"),
        "the stub request must send the TARGET connection's `target_script` — \
         the receiver lives on the node the connection points at (the scene \
         root by default), and that field is the only place its script path is \
         known. Call site: {call_site}",
    );
    assert!(
        call_site.contains("method: method"),
        "and the method the user typed, so the stub's name matches the \
         connection. Call site: {call_site}",
    );

    // It must be reached FROM the connect flow, after the connection exists —
    // `target_script` is a per-connection field, so the call cannot be hoisted
    // ahead of the POST that creates the connection.
    let connect_at = EDITOR_HTML
        .find("/api/node/signals/connect")
        .expect("the connect route is served");
    assert!(
        at > connect_at,
        "the stub request must follow the connect call (connect at \
         {connect_at}, autocreate at {at})",
    );
}
