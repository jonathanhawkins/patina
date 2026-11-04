//! pat-mgixk.111 — the inspector's resource picker can Make Unique and Save As.
//!
//! ⚠️ **The filename is deliberately short; do not "restore" the descriptive
//! one.** The acceptance criterion names the test **function**
//! (`inspector_resource_picker_make_unique_save_as`), which is spelled out
//! below — `--test` names the **file**. The verifier truncates a reported test
//! command at 58 characters, and a command built from the function name is 81,
//! so it would be cut mid-name into a target that does not exist and fail
//! looking like a missing test. The sibling `res_picker_e2e.rs` carries the same
//! warning because that is what parked its bead twice. `--test res_toolbar_e2e`
//! is 52 and survives.
//!
//! # What was wrong
//!
//! `openMenu` (editor_ui.rs) built exactly three rows — `New <Type> …`,
//! `Quick Load …` and `Clear` — so Godot's `Make Unique`, `Save`, `Save As`,
//! `Copy`, `Paste` and `Show in FileSystem`
//! (`editor/inspector/editor_resource_picker.cpp:327-360`) had no counterpart at
//! all. `grep make_unique editor_ui.rs` found nothing, and
//! `POST /api/property/resource/save_as` had existed server-side since
//! pat-0q2kx.13 with **zero** client callers.
//!
//! # The path driven
//!
//! ```text
//! served editor HTML (picker menu row)
//!   -> POST /api/property/resource/make_unique   (new in this bead)
//!   -> POST /api/property/resource/save_as       (existed, never called)
//!     -> EditorCommand::SetProperty (so both are undoable)
//!       -> POST /api/scene/save -> the .tscn on disk
//! ```
//!
//! Asserted through the real HTTP handlers on a real socket, which is how the
//! served editor reaches them.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Harness (same shape as the sibling res_picker_e2e.rs)
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

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

/// Process-unique fixture dir — fixed `/tmp` paths collide across concurrent
/// test processes in the verifier lane.
fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_res_toolbar_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The `SubResource("…")` id a node's property currently references.
fn sub_id(resp: &str) -> Option<String> {
    let rest = resp.split("\"id\":\"").nth(1)?;
    Some(rest.split('"').next()?.to_string())
}

// ---------------------------------------------------------------------------
// (1) Static wiring guard
// ---------------------------------------------------------------------------

/// The served HTML must actually call the two routes.
///
/// Without this half the functional half below would still pass against a
/// picker whose menu never grew the rows — the endpoints would work and no user
/// could reach them, which is exactly the state this bead found them in.
#[test]
fn editor_html_wires_make_unique_and_save_as() {
    for needle in [
        "/api/property/resource/make_unique",
        "/api/property/resource/save_as",
        "function makeResourceUnique",
        "function saveResourceAs",
    ] {
        assert!(
            EDITOR_HTML.contains(needle),
            "served HTML is missing resource-toolbar wiring: {needle}"
        );
    }

    // Narrow to the picker's own menu builder, so a row added elsewhere in the
    // 20k-line file cannot make this pass.
    let menu = EDITOR_HTML
        .split("function openMenu(anchor)")
        .nth(1)
        .expect("openMenu exists")
        .split("main.addEventListener")
        .next()
        .expect("openMenu body is delimited");

    for needle in [
        "Make Unique",
        "Save As",
        "makeResourceUnique(",
        "saveResourceAs(",
    ] {
        assert!(
            menu.contains(needle),
            "the picker menu itself must offer {needle}; before this bead it \
             built only New/Quick Load/Clear"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) THE ACCEPTANCE TEST
// ---------------------------------------------------------------------------

/// **THE ACCEPTANCE TEST (pat-mgixk.111).**
///
/// Two `Sprite2D`s share one inline `CanvasTexture`; Make Unique on one splits
/// it, and Save As turns the other into an external reference.
#[test]
fn inspector_resource_picker_make_unique_save_as() {
    let dir = unique_dir("mkuniq");
    let scene_path = dir.join("main.tscn");

    // Scene: Root -> { A, B }, both Sprite2D referencing the SAME inline
    // CanvasTexture. This is the shape a `.tscn` with one `[sub_resource]`
    // referenced twice loads into: the reference on the property plus the
    // `__resref__<prop>` = `Class::id` companion the saver rebuilds the header
    // from.
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = tree.add_child(root, Node::new("Root", "Node2D")).unwrap();
    let a = tree
        .add_child(scene_root, Node::new("A", "Sprite2D"))
        .unwrap();
    let b = tree
        .add_child(scene_root, Node::new("B", "Sprite2D"))
        .unwrap();
    for nid in [a, b] {
        let node = tree.get_node_mut(nid).unwrap();
        node.set_property("texture", Variant::String("SubResource(\"1_tex\")".into()));
        node.set_property(
            "__resref__texture",
            Variant::String("CanvasTexture::1_tex".into()),
        );
    }
    let (a_raw, b_raw) = (a.raw(), b.raw());

    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());

    let port = free_port();
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // -- (a) Make Unique on A splits the shared resource --------------------
    let resp = http_post(
        port,
        "/api/property/resource/make_unique",
        &format!(r#"{{"node_id":{a_raw},"property":"texture"}}"#),
    );
    assert!(
        resp.contains("\"ok\":true"),
        "Make Unique on a SHARED inline resource must succeed; before this bead \
         the route did not exist at all:\n{resp}"
    );
    let new_id = sub_id(&resp).expect("the response must name the new id");
    assert_ne!(
        new_id, "1_tex",
        "Make Unique must mint a NEW id; reusing the shared one leaves both \
         nodes pointing at one [sub_resource] and is the bug"
    );

    // -- (b) …and B is untouched -------------------------------------------
    {
        let st = handle.state().lock().unwrap();
        // Resolved by WALK rather than by minting a `NodeId` from the raw id —
        // the same thing every `node_id` endpoint in editor_server.rs does, and
        // it avoids depending on a raw-id constructor being public.
        let mut ids = Vec::new();
        st.scene_tree
            .collect_subtree_top_down(st.scene_tree.root_id(), &mut ids);
        let b_id = ids
            .into_iter()
            .find(|i| i.raw() == b_raw)
            .expect("B is still in the tree");
        let b_node = st.scene_tree.get_node(b_id).expect("B has a node");
        assert_eq!(
            b_node.get_property("texture"),
            Variant::String("SubResource(\"1_tex\")".into()),
            "Make Unique on A must not touch B — if B moved too, the operation \
             re-pointed the shared resource instead of splitting it"
        );
    }

    // -- (c) a SECOND Make Unique on A is refused ---------------------------
    //
    // 🔑 The discriminator. Godot DISABLES the menu item when the resource is
    // internal and referenced once — "This Resource is already unique."
    // (editor_resource_picker.cpp:337-338). The identical request that just
    // succeeded must now fail, which is what proves the route counts references
    // rather than always minting an id.
    let resp = http_post(
        port,
        "/api/property/resource/make_unique",
        &format!(r#"{{"node_id":{a_raw},"property":"texture"}}"#),
    );
    assert!(
        resp.contains("already unique"),
        "a second Make Unique on a now-unshared resource must be refused, as \
         Godot greys the item out; minting another id here would fill the undo \
         history with no-ops:\n{resp}"
    );

    // -- (d) Save As on B writes a .tres and repoints the property ----------
    let resp = http_post(
        port,
        "/api/property/resource/save_as",
        &format!(r#"{{"node_id":{b_raw},"property":"texture","path":"res://tex.tres"}}"#),
    );
    assert!(
        resp.contains("\"ok\":true"),
        "Save As must succeed on B's inline resource:\n{resp}"
    );
    assert!(
        dir.join("tex.tres").exists(),
        "Save As must write the .tres to disk"
    );

    // -- (e) the saved scene shows the split, in Godot's on-disk form -------
    let resp = http_post(
        port,
        "/api/scene/save",
        &format!(
            r#"{{"path":"{}"}}"#,
            scene_path.to_string_lossy().replace('\\', "\\\\")
        ),
    );
    assert!(!resp.is_empty(), "save returned nothing");
    let saved = std::fs::read_to_string(&scene_path).expect("the scene was written");

    assert!(
        saved.contains(&format!("SubResource(\"{new_id}\")")),
        "A must reference its OWN new sub-resource in the saved scene:\n{saved}"
    );
    assert!(
        saved.contains("texture = SubResource("),
        "A's property must still be an inline SubResource reference:\n{saved}"
    );
    // B went external in (d). Asserted in Godot's exact on-disk form, not just
    // as "the filename appears somewhere": the sibling test documents that
    // `contains(\"…tres\")` alone passed while the feature was broken, because
    // repointing the property at a bare `res://…` string puts that text in the
    // file as a plain String property with no `[ext_resource]` header, which
    // Godot loads as a String rather than a resource.
    assert!(
        saved.contains("[ext_resource") && saved.contains("tex.tres"),
        "B's Save As must emit an [ext_resource] header for the .tres:\n{saved}"
    );
    assert!(
        saved.contains("texture = ExtResource("),
        "B's property must be an ExtResource reference, not a bare path string:\n{saved}"
    );
    // Bookkeeping companions are internal and must never reach the file.
    assert!(
        !saved.contains("__resref__") && !saved.contains("__extres__"),
        "internal companions leaked into the saved scene:\n{saved}"
    );

    drop(handle);
}
