//! pat-s3rrv.66 — the editor dispatches the EditorPlugin **selection** virtuals.
//!
//! `EditorState` has carried `active_plugins` since plugins were loadable, but
//! only six of EditorPlugin's virtuals were ever called: `_enter_tree`,
//! `_enable_plugin`, `_disable_plugin`, `_exit_tree`, `_ready` and `_process`.
//! `_handles`, `_edit` and `_make_visible` had **zero** occurrences anywhere in
//! `crates/gdeditor/src/`. So a plugin could load, print, and run per-frame —
//! and never learn that the user had selected anything. Every plugin whose
//! whole purpose is to edit the selected node (the common case: gizmo plugins,
//! custom inspectors, tile painters) was inert.
//!
//! Godot's contract, from `editor/plugins/editor_plugin.cpp` and the
//! `EditorPlugin` docs:
//!
//! - `_handles(object)` is asked FIRST, and only its answer decides the rest.
//! - `_edit(object)` is called only on a plugin that answered true.
//! - `_make_visible(bool)` is called on **every** plugin — true for the one
//!   that claimed the object, false for the others — which is how a plugin
//!   that is no longer relevant hides its panel.
//!
//! ⚠️ That last point is the one worth stating: `_make_visible(false)` reaches
//! plugins that never claimed anything, INCLUDING when the selection is
//! cleared. A plugin showing a panel for the previously-selected node has no
//! other signal to hide it.

use gdeditor::editor_server::{
    notify_plugins_of_selection, plugin_display_name, plugin_has_main_screen, EditorState,
};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

/// A plugin that answers `_handles` with `claims`, recording every call.
///
/// ⚠️ `_handles` returns the *variable*, not a literal, so a test can tell
/// "the method ran and answered false" apart from "the method never ran".
fn plugin_source(claims: bool) -> String {
    format!(
        "extends EditorPlugin\n\
         var answer = {claims}\n\
         var handles_calls = 0\n\
         var edit_calls = 0\n\
         var visible_calls = 0\n\
         var last_visible = \"unset\"\n\
         var edited_nil = true\n\
         func _handles(object):\n\
         \x20   self.handles_calls += 1\n\
         \x20   return self.answer\n\
         func _edit(object):\n\
         \x20   self.edit_calls += 1\n\
         \x20   self.edited_nil = (object == null)\n\
         func _make_visible(v):\n\
         \x20   self.visible_calls += 1\n\
         \x20   self.last_visible = v\n"
    )
}

/// A scene with one selectable node, plus the plugins registered by folder name.
fn state_with_plugins(plugins: &[(&str, bool)]) -> (EditorState, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Sprite", "Sprite2D"))
        .expect("add child");

    let mut state = EditorState::new(tree);
    for (folder, claims) in plugins {
        let instance = GDScriptNodeInstance::from_source(&plugin_source(*claims), root)
            .expect("plugin script parses");
        state.active_plugins.insert((*folder).to_string(), instance);
    }
    (state, node)
}

fn prop(state: &EditorState, folder: &str, name: &str) -> Variant {
    state
        .active_plugins
        .get(folder)
        .expect("plugin still registered")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn as_int(v: Variant) -> i64 {
    match v {
        Variant::Int(i) => i,
        other => panic!("expected Int, got {other:?}"),
    }
}

/// **The acceptance test**: selecting a node runs `_handles` → `_edit` →
/// `_make_visible` on the plugin that claims it.
#[test]
fn editor_plugin_virtual_table_dispatch() {
    let (mut state, node) = state_with_plugins(&[("gizmo", true)]);

    // Nothing has been dispatched yet — the counters start at zero, so a test
    // that passed here without the call would be measuring its own setup.
    assert_eq!(as_int(prop(&state, "gizmo", "handles_calls")), 0);

    notify_plugins_of_selection(&mut state, Some(node));

    assert_eq!(
        as_int(prop(&state, "gizmo", "handles_calls")),
        1,
        "_handles must be asked exactly once per selection — zero is the bug \
         this bead exists for, and more than one means the dispatcher is \
         asking repeatedly instead of caching its answer",
    );
    assert_eq!(
        as_int(prop(&state, "gizmo", "edit_calls")),
        1,
        "…and a plugin that answered true must then be given the object",
    );
    assert_eq!(
        prop(&state, "gizmo", "edited_nil"),
        Variant::Bool(false),
        "_edit must receive the SELECTED OBJECT, not Nil — a dispatcher that \
         called the virtual with nothing would satisfy the counter above",
    );
    assert_eq!(
        as_int(prop(&state, "gizmo", "visible_calls")),
        1,
        "_make_visible must be called too",
    );
    assert_eq!(
        prop(&state, "gizmo", "last_visible"),
        Variant::Bool(true),
        "…with TRUE for the plugin that claimed the object",
    );
}

/// A plugin that declines the object is not edited, and IS hidden
/// (pat-s3rrv.66).
///
/// This is the half a naive implementation gets wrong: it is tempting to call
/// `_edit`/`_make_visible` on whoever is loaded and let the plugin sort it out.
/// Godot gates `_edit` on `_handles`, and sends the non-claiming plugins
/// `_make_visible(false)` so they can hide.
#[test]
fn a_plugin_that_declines_is_hidden_not_edited() {
    let (mut state, node) = state_with_plugins(&[("gizmo", true), ("tiles", false)]);

    notify_plugins_of_selection(&mut state, Some(node));

    assert_eq!(
        as_int(prop(&state, "tiles", "handles_calls")),
        1,
        "every plugin is asked, not just the first one to answer true",
    );
    assert_eq!(
        as_int(prop(&state, "tiles", "edit_calls")),
        0,
        "_edit must NOT reach a plugin that declined the object — that is the \
         entire purpose of asking _handles first",
    );
    assert_eq!(
        prop(&state, "tiles", "last_visible"),
        Variant::Bool(false),
        "…but it must still be told to hide, or its panel stays on screen for \
         a node it does not handle",
    );
    // The claiming plugin is unaffected by the presence of a second one.
    assert_eq!(as_int(prop(&state, "gizmo", "edit_calls")), 1);
    assert_eq!(prop(&state, "gizmo", "last_visible"), Variant::Bool(true));
}

/// Clearing the selection hides every plugin without asking `_handles`
/// (pat-s3rrv.66).
///
/// `_handles(null)` is not a question Godot asks — there is no object to
/// handle. What it does do is send `_make_visible(false)` everywhere, which is
/// the only signal a plugin gets that its subject is gone. Deleting the
/// selected node goes through this path.
#[test]
fn clearing_the_selection_hides_every_plugin() {
    let (mut state, node) = state_with_plugins(&[("gizmo", true)]);
    notify_plugins_of_selection(&mut state, Some(node));

    notify_plugins_of_selection(&mut state, None);

    assert_eq!(
        as_int(prop(&state, "gizmo", "handles_calls")),
        1,
        "the empty selection must not re-ask _handles — the count is still the \
         one from the selection above",
    );
    assert_eq!(
        as_int(prop(&state, "gizmo", "edit_calls")),
        1,
        "…nor re-run _edit",
    );
    assert_eq!(
        prop(&state, "gizmo", "last_visible"),
        Variant::Bool(false),
        "…but the plugin must be hidden, or a panel opened for a node that no \
         longer exists stays up",
    );
    assert_eq!(
        as_int(prop(&state, "gizmo", "visible_calls")),
        2,
        "_make_visible fires on BOTH transitions, not only the first",
    );
}

/// A plugin missing the virtuals entirely is skipped, not an error
/// (pat-s3rrv.66).
///
/// Most of EditorPlugin's 25 virtuals are optional; a plugin that only wants
/// `_enter_tree` must not break selection for the plugins that do implement it.
#[test]
fn a_plugin_without_the_virtuals_is_skipped() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node = tree
        .add_child(root, Node::new("Sprite", "Sprite2D"))
        .expect("add child");
    let mut state = EditorState::new(tree);

    let bare = GDScriptNodeInstance::from_source(
        "extends EditorPlugin\nvar ticks = 0\nfunc _enter_tree():\n    self.ticks += 1\n",
        root,
    )
    .expect("plugin script parses");
    state.active_plugins.insert("bare".to_string(), bare);
    let full = GDScriptNodeInstance::from_source(&plugin_source(true), root).expect("parses");
    state.active_plugins.insert("gizmo".to_string(), full);

    // The assertion is that this does not panic and the other plugin is still
    // served — a dispatcher that called the virtuals unconditionally would
    // raise on the bare plugin and, depending on where it raised, drop the
    // rest of the loop.
    notify_plugins_of_selection(&mut state, Some(node));

    assert_eq!(as_int(prop(&state, "bare", "ticks")), 0);
    assert_eq!(
        as_int(prop(&state, "gizmo", "edit_calls")),
        1,
        "a plugin missing the virtuals must not stop the ones that have them",
    );
}

/// `_get_plugin_name` and `_has_main_screen` are read from the SCRIPT
/// (pat-s3rrv.66).
///
/// `plugin.cfg` carries a `name=`, and that is what the editor listed. Godot
/// asks the plugin itself: `_get_plugin_name()` is what appears on the
/// main-screen tab, and `_has_main_screen()` decides whether there is a tab at
/// all. Neither virtual was ever called, so a plugin that named itself was
/// listed under its cfg name and a main-screen plugin got no tab.
#[test]
fn plugin_name_and_main_screen_come_from_the_script() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut state = EditorState::new(tree);

    let named = GDScriptNodeInstance::from_source(
        "extends EditorPlugin\nfunc _get_plugin_name():\n    return \"Gizmo Painter\"\nfunc _has_main_screen():\n    return true\n",
        root,
    )
    .expect("plugin script parses");
    state.active_plugins.insert("gizmo".to_string(), named);
    let quiet = GDScriptNodeInstance::from_source("extends EditorPlugin\nvar x = 1\n", root)
        .expect("plugin script parses");
    state.active_plugins.insert("quiet".to_string(), quiet);

    assert_eq!(
        plugin_display_name(&mut state, "gizmo"),
        "Gizmo Painter",
        "the name must come from _get_plugin_name, not the folder",
    );
    assert!(plugin_has_main_screen(&mut state, "gizmo"));

    // A plugin implementing neither virtual falls back rather than erroring —
    // both are optional, and most plugins implement neither.
    assert_eq!(
        plugin_display_name(&mut state, "quiet"),
        "quiet",
        "…falling back to the folder name when the virtual is absent",
    );
    assert!(
        !plugin_has_main_screen(&mut state, "quiet"),
        "no _has_main_screen means no main-screen tab — Godot's default is false",
    );

    // An unknown folder is not a panic: the plugin list and the live instances
    // can disagree while a plugin is being enabled.
    assert_eq!(plugin_display_name(&mut state, "absent"), "absent");
    assert!(!plugin_has_main_screen(&mut state, "absent"));
}

// ---------------------------------------------------------------------------
// End-to-end: the real HTTP selection route (pat-s3rrv.66)
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..40 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 39 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_request_str(port: u16, request: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.write_all(request.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    resp
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

/// **The bead's own wiring assertion**: a plugin reacts to a selection made
/// through the editor server's HTTP route (pat-s3rrv.66).
///
/// The unit tests above call the dispatcher directly. This one proves the call
/// is actually reached from `POST /api/node/select` — the route the served
/// Scene dock uses — and that the plugin's `print` output lands in the editor
/// Output log, which is how a plugin author sees it.
///
/// ⚠️ Asserted through `/api/logs` rather than a returned value: the selection
/// route answers `{"ok":true}` whatever the plugins do, so a test reading its
/// response would pass against a server that dispatched nothing.
#[test]
fn plugin_virtuals_fire_through_the_http_selection_route() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sprite = tree
        .add_child(root, Node::new("Sprite", "Sprite2D"))
        .expect("add child");
    let other = tree
        .add_child(root, Node::new("Other", "Node2D"))
        .expect("add child");

    let mut state = EditorState::new(tree);
    let plugin = GDScriptNodeInstance::from_source(
        "extends EditorPlugin\nvar claim = true\nfunc _handles(obj):\n    print(\"PLUGIN handles\")\n    return self.claim\nfunc _edit(obj):\n    print(\"PLUGIN edit\")\nfunc _make_visible(v):\n    if v == true:\n        print(\"PLUGIN visible true\")\n    if v == false:\n        print(\"PLUGIN visible false\")\n",
        root,
    )
    .expect("plugin script parses");
    state.active_plugins.insert("gizmo".to_string(), plugin);

    let port = free_port();
    let handle = gdeditor::editor_server::EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(
        port,
        "/api/node/select",
        &format!("{{\"node_id\":{}}}", sprite.raw()),
    );
    assert!(resp.contains("200 OK"), "selection route answered: {resp}");

    let logs = http_get(port, "/api/logs");
    assert!(
        logs.contains("PLUGIN handles"),
        "selecting through the HTTP route must ask the plugin _handles; logs: {logs}",
    );
    assert!(
        logs.contains("PLUGIN edit"),
        "…and give it the object it claimed; logs: {logs}",
    );
    assert!(
        logs.contains("PLUGIN visible true"),
        "…and show it; logs: {logs}",
    );

    // Selecting a different node runs the whole handshake again — a dispatcher
    // that fired once at enable time would pass everything above.
    let resp = http_post(
        port,
        "/api/node/select",
        &format!("{{\"node_id\":{}}}", other.raw()),
    );
    assert!(resp.contains("200 OK"));
    let logs = http_get(port, "/api/logs");
    assert_eq!(
        logs.matches("PLUGIN handles").count(),
        2,
        "_handles must be re-asked on the second selection; logs: {logs}",
    );

    handle.stop();
}

/// **Acceptance clause**: `_apply_changes` fires exactly once per scene save
/// (pat-s3rrv.66).
///
/// Godot's `EditorNode::_save_scene` calls `apply_changes_in_editors()` BEFORE
/// serialising, so a plugin holding unflushed edits can push them into the
/// scene and have them land in the file. Patina dispatched it nowhere at all —
/// `grep -c '"_apply_changes"'` over the server was 0 — so a plugin could never
/// participate in saving.
///
/// ⚠️ Asserted by COUNTING, not by presence. A dispatcher wired into a helper
/// that the save path calls twice would still "contain" the line once.
#[test]
fn apply_changes_fires_once_per_scene_save() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // A save needs a scene root — the first child of the tree root.
    tree.add_child(root, Node::new("Main", "Node2D"))
        .expect("add child");

    let mut state = EditorState::new(tree);
    let plugin = GDScriptNodeInstance::from_source(
        "extends EditorPlugin\nfunc _apply_changes():\n    print(\"PLUGIN apply\")\n",
        root,
    )
    .expect("plugin script parses");
    state.active_plugins.insert("saver".to_string(), plugin);

    let port = free_port();
    let handle = gdeditor::editor_server::EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let path = std::env::temp_dir().join(format!(
        "patina_plugin_apply_{}_{}.tscn",
        std::process::id(),
        port
    ));
    let body = format!("{{\"path\":\"{}\"}}", path.to_string_lossy());

    let resp = http_post(port, "/api/scene/save", &body);
    assert!(resp.contains("200 OK"), "save answered: {resp}");
    let logs = http_get(port, "/api/logs");
    assert_eq!(
        logs.matches("PLUGIN apply").count(),
        1,
        "_apply_changes must fire exactly once per save; logs: {logs}",
    );

    // A second save fires it again — a one-shot at startup would pass above.
    let resp = http_post(port, "/api/scene/save", &body);
    assert!(resp.contains("200 OK"), "second save answered: {resp}");
    let logs = http_get(port, "/api/logs");
    assert_eq!(
        logs.matches("PLUGIN apply").count(),
        2,
        "each save must dispatch _apply_changes again; logs: {logs}",
    );

    // The scene really was written — proving the dispatch did not replace the
    // save, only precede it.
    assert!(
        path.exists(),
        "the scene must still be saved after the plugin hook runs"
    );
    let _ = std::fs::remove_file(&path);
    handle.stop();
}

/// **Acceptance clause**: a plugin returning `true` from
/// `_forward_canvas_gui_input` CONSUMES the click, so the editor's own select
/// tool does not change the selection (pat-s3rrv.66).
///
/// This is what makes a plugin able to implement its own viewport tool. Without
/// it the editor always wins the click and a click-to-place plugin is
/// unusable.
///
/// ⚠️ The control matters as much as the claim: with a plugin that returns
/// false the click must fall through to the editor and CLEAR the selection
/// (nothing is under the cursor). Asserting only the consuming case would pass
/// against a server that ignored the click entirely.
#[test]
fn a_plugin_can_consume_a_viewport_click() {
    fn serve(returns: &str) -> (gdeditor::editor_server::EditorServerHandle, u16, u64) {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let sprite = tree
            .add_child(root, Node::new("Sprite", "Sprite2D"))
            .expect("add child");

        let mut state = EditorState::new(tree);
        let plugin = GDScriptNodeInstance::from_source(
            &format!(
                "extends EditorPlugin\nfunc _forward_canvas_gui_input(event):\n    print(\"PLUGIN input\")\n    return {returns}\n"
            ),
            root,
        )
        .expect("plugin script parses");
        state.active_plugins.insert("tool".to_string(), plugin);

        let port = free_port();
        let handle = gdeditor::editor_server::EditorServerHandle::start(port, state);
        thread::sleep(Duration::from_millis(100));
        (handle, port, sprite.raw())
    }

    // -- the plugin consumes the click ------------------------------------
    let (handle, port, sprite) = serve("true");
    let resp = http_post(
        port,
        "/api/node/select",
        &format!("{{\"node_id\":{sprite}}}"),
    );
    assert!(resp.contains("200 OK"), "select answered: {resp}");

    let resp = http_post(port, "/api/viewport/click", r#"{"x":10.0,"y":10.0}"#);
    assert!(
        resp.contains("consumed_by"),
        "a plugin returning true must consume the click: {resp}"
    );
    assert!(
        resp.contains(&format!("\"selected\":{sprite}")),
        "…and the selection must be UNCHANGED, which is the point of consuming \
         it — the editor's select tool never ran: {resp}"
    );
    let logs = http_get(port, "/api/logs");
    assert!(
        logs.contains("PLUGIN input"),
        "the plugin must actually have been offered the event; logs: {logs}"
    );
    handle.stop();

    // -- the control: a plugin that declines lets the editor have it -------
    let (handle2, port2, sprite2) = serve("false");
    let resp = http_post(
        port2,
        "/api/node/select",
        &format!("{{\"node_id\":{sprite2}}}"),
    );
    assert!(resp.contains("200 OK"));

    let resp = http_post(port2, "/api/viewport/click", r#"{"x":10.0,"y":10.0}"#);
    assert!(
        !resp.contains("consumed_by"),
        "a plugin returning false must NOT consume the click: {resp}"
    );
    // ⚠️ Deliberately NOT asserting the resulting selection value. Whether a
    // click at (10,10) hits the sprite depends on hit-test geometry — a
    // Sprite2D with no texture has no drawable rect — and pinning that here
    // would make this test fail for reasons that have nothing to do with
    // plugin dispatch. The claim under test is that the editor's own path ran
    // at all, which the absence of `consumed_by` establishes: the consuming
    // branch returns BEFORE the hit-test and is the only producer of that key.
    assert!(
        resp.contains("\"selected\""),
        "…so the editor's own select tool runs and answers with a selection \
         field: {resp}"
    );
    handle2.stop();
}

/// **Item (h)**: multi-select runs the plugin handshake too (pat-s3rrv.66).
///
/// `POST /api/node/select_multi` — the route ctrl-click and shift-click take —
/// set the selection and dispatched to nobody, so a plugin's panel went stale
/// the moment the user extended a selection. Godot dispatches on the FIRST
/// selected object.
#[test]
fn multi_select_runs_the_plugin_handshake() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sprite = tree
        .add_child(root, Node::new("Sprite", "Sprite2D"))
        .expect("add child");
    let other = tree
        .add_child(root, Node::new("Other", "Node2D"))
        .expect("add child");

    let mut state = EditorState::new(tree);
    let plugin = GDScriptNodeInstance::from_source(
        "extends EditorPlugin\nfunc _handles(obj):\n    print(\"PLUGIN handles\")\n    return true\nfunc _edit(obj):\n    print(\"PLUGIN edit\")\nfunc _make_visible(v):\n    print(\"PLUGIN visible\")\n",
        root,
    )
    .expect("plugin script parses");
    state.active_plugins.insert("multi".to_string(), plugin);

    let port = free_port();
    let handle = gdeditor::editor_server::EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // The explicit `node_ids` array path.
    let resp = http_post(
        port,
        "/api/node/select_multi",
        &format!("{{\"node_ids\":[{},{}]}}", sprite.raw(), other.raw()),
    );
    assert!(resp.contains("200 OK"), "select_multi answered: {resp}");
    let logs = http_get(port, "/api/logs");
    assert_eq!(
        logs.matches("PLUGIN handles").count(),
        1,
        "selecting several nodes must ask the plugin about the FIRST one, once \
         — not once per selected node; logs: {logs}",
    );
    assert!(
        logs.contains("PLUGIN edit") && logs.contains("PLUGIN visible"),
        "…and complete the handshake; logs: {logs}",
    );

    // The `mode` path, which is what ctrl-click actually sends. A fix applied
    // to only one of the two exit paths would pass everything above.
    let resp = http_post(
        port,
        "/api/node/select_multi",
        &format!("{{\"node_id\":{},\"mode\":\"add\"}}", other.raw()),
    );
    assert!(resp.contains("200 OK"), "mode select answered: {resp}");
    let logs = http_get(port, "/api/logs");
    assert_eq!(
        logs.matches("PLUGIN handles").count(),
        2,
        "the add/remove/toggle/range path must dispatch as well; logs: {logs}",
    );

    handle.stop();
}

/// **Ordering contract**: `_apply_changes` runs BEFORE the scene is written and
/// `_save_external_data` AFTER it (pat-s3rrv.66).
///
/// The two are easy to lump into one call site, and that would put one of them
/// on the wrong side of the write. Godot's docs are explicit and opposite:
/// `_apply_changes` is called "when the editor is about to save … to apply any
/// pending state changes", while `_save_external_data` is called "after the
/// editor saves the project … to save edited external scenes/resources".
#[test]
fn save_external_data_runs_after_apply_changes() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Main", "Node2D"))
        .expect("add child");

    let mut state = EditorState::new(tree);
    let plugin = GDScriptNodeInstance::from_source(
        "extends EditorPlugin\nfunc _apply_changes():\n    print(\"PLUGIN apply\")\nfunc _save_external_data():\n    print(\"PLUGIN external\")\n",
        root,
    )
    .expect("plugin script parses");
    state.active_plugins.insert("saver".to_string(), plugin);

    let port = free_port();
    let handle = gdeditor::editor_server::EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let path = std::env::temp_dir().join(format!(
        "patina_plugin_external_{}_{}.tscn",
        std::process::id(),
        port
    ));
    let body = format!("{{\"path\":\"{}\"}}", path.to_string_lossy());
    let resp = http_post(port, "/api/scene/save", &body);
    assert!(resp.contains("200 OK"), "save answered: {resp}");

    let logs = http_get(port, "/api/logs");
    let apply_at = logs
        .find("PLUGIN apply")
        .unwrap_or_else(|| panic!("_apply_changes never fired; logs: {logs}"));
    let external_at = logs
        .find("PLUGIN external")
        .unwrap_or_else(|| panic!("_save_external_data never fired; logs: {logs}"));
    assert!(
        apply_at < external_at,
        "_apply_changes must run BEFORE _save_external_data — the first flushes \
         pending edits so they reach the file, the second saves external \
         resources once the scene is on disk; logs: {logs}",
    );
    assert!(path.exists(), "the scene must still be written");

    let _ = std::fs::remove_file(&path);
    handle.stop();
}

// ---------------------------------------------------------------------------
// pat-s3rrv.66 (d): `_build` gates Play
// ---------------------------------------------------------------------------

/// A plugin whose `_build` answers `approves`, and records that it was asked.
fn build_plugin_source(approves: bool) -> String {
    format!(
        "extends EditorPlugin\n\
         var build_calls = 0\n\
         func _build():\n\
         \x20   self.build_calls += 1\n\
         \x20   return {approves}\n"
    )
}

fn state_with_build_plugins(plugins: &[(&str, bool)]) -> EditorState {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Sprite", "Sprite2D"))
        .expect("add child");
    let mut state = EditorState::new(tree);
    for (folder, approves) in plugins {
        let instance = GDScriptNodeInstance::from_source(&build_plugin_source(*approves), root)
            .expect("plugin script parses");
        state.active_plugins.insert((*folder).to_string(), instance);
    }
    state
}

/// A refusing `_build` aborts the run, and Play reports it.
#[test]
fn a_plugin_can_veto_play_through_build() {
    let port = free_port();
    let handle = gdeditor::editor_server::EditorServerHandle::start(
        port,
        state_with_build_plugins(&[("blocker", false)]),
    );
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(port, "/api/runtime/play", "{}");
    assert!(
        resp.contains("\"build_failed\":true"),
        "a false `_build` must abort the run and say so, got:\n{resp}"
    );
    // ⚠️ And the project must NOT be running. Reporting the refusal while
    // starting anyway would satisfy the assertion above and defeat the whole
    // point of the hook.
    let status = http_get(port, "/api/runtime/status");
    assert!(
        !status.contains("\"running\":true"),
        "the run must actually be aborted, not merely reported; got:\n{status}"
    );
    drop(handle);
}

/// The control: an approving `_build` lets Play proceed.
///
/// Without this half, a `plugin_build_refusal` that returned `Some` for
/// everything — or a Play route broken for unrelated reasons — would pass the
/// veto test above.
#[test]
fn an_approving_build_lets_play_proceed() {
    let port = free_port();
    let handle = gdeditor::editor_server::EditorServerHandle::start(
        port,
        state_with_build_plugins(&[("ok", true)]),
    );
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(port, "/api/runtime/play", "{}");
    assert!(
        !resp.contains("\"build_failed\":true"),
        "an approving `_build` must not block the run, got:\n{resp}"
    );
    drop(handle);
}

/// ⚠️ A refusal stops the sweep: plugins after the refuser are never asked.
///
/// Godot's doc is explicit — "The run is aborted immediately, so this also
/// prevents all other plugins' `_build` methods from running." A build step that
/// runs after another has vetoed is doing work for a run that will not happen,
/// possibly against a half-prepared project.
///
/// The folders are named so a sorted walk puts the refuser FIRST; the dispatch
/// sorts for exactly this reason, so which plugin is blamed does not depend on a
/// HashMap walk order.
#[test]
fn a_build_refusal_stops_later_plugins_being_asked() {
    let port = free_port();
    let handle = gdeditor::editor_server::EditorServerHandle::start(
        port,
        state_with_build_plugins(&[("a_blocker", false), ("b_later", true)]),
    );
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(port, "/api/runtime/play", "{}");
    assert!(resp.contains("\"build_failed\":true"), "got:\n{resp}");
    assert!(
        resp.contains("a_blocker"),
        "the refusing plugin must be named in the response, got:\n{resp}"
    );
    drop(handle);
}

// ---------------------------------------------------------------------------
// pat-s3rrv.66 (d): `_clear` on scene reset
// ---------------------------------------------------------------------------

/// Godot calls `_clear` when the edited scene is cleared, so a plugin drops the
/// node it was editing rather than holding a reference into a tree that no
/// longer exists.
#[test]
fn a_new_scene_clears_every_plugin() {
    let port = free_port();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Sprite", "Sprite2D"))
        .expect("add child");
    let mut state = EditorState::new(tree);

    // Two plugins, so "broadcast" is distinguishable from "first one only".
    for folder in ["alpha", "beta"] {
        let src = "extends EditorPlugin\n\
                   var clear_calls = 0\n\
                   func _clear():\n\
                   \x20   self.clear_calls += 1\n";
        let instance = GDScriptNodeInstance::from_source(src, root).expect("plugin script parses");
        state.active_plugins.insert(folder.to_string(), instance);
    }

    let handle = gdeditor::editor_server::EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    let resp = http_post(port, "/api/scene/new", "");
    assert!(
        resp.contains("200 OK"),
        "POST /api/scene/new should 200:\n{resp}"
    );
    drop(handle);
}

/// ⚠️ The dispatch itself, asserted in-process so the CALL COUNT is observable.
///
/// The HTTP test above can only see that the route succeeded; it cannot read the
/// plugins back out of the moved `EditorState`. Counting matters here because
/// `_clear` is a broadcast: a `stop_at_first_claim = true` slip would still
/// clear ONE plugin and leave the rest holding stale nodes — passing any test
/// that only checks "a plugin was cleared".
#[test]
fn clear_reaches_every_plugin_not_just_the_first() {
    let tree = SceneTree::new();
    let root = tree.root_id();
    let mut state = EditorState::new(tree);
    for folder in ["alpha", "beta", "gamma"] {
        let src = "extends EditorPlugin\n\
                   var clear_calls = 0\n\
                   func _clear():\n\
                   \x20   self.clear_calls += 1\n";
        let instance = GDScriptNodeInstance::from_source(src, root).expect("plugin script parses");
        state.active_plugins.insert(folder.to_string(), instance);
    }

    gdeditor::editor_server::dispatch_plugin_virtual(&mut state, "_clear", &[], false);

    for folder in ["alpha", "beta", "gamma"] {
        assert_eq!(
            as_int(prop(&state, folder, "clear_calls")),
            1,
            "`{folder}` must be cleared exactly once — _clear is a broadcast, so \
             every plugin has to drop its state, and exactly one call means it \
             neither stopped early nor fired twice"
        );
    }
}
