//! pat-mgixk.110 acceptance: the Scene dock's branch operations, end to end
//! through the editor server.
//!
//! # What was wrong
//!
//! The scene-tree context menu had no Save Branch as Scene, Reparent to New
//! Node or Make Scene Root entry; Detach Script and Extend Script were
//! `showFsToast` stubs; and `POST /api/node/save_branch` (which already worked)
//! had **no client caller at all**. A grep for `make_scene_root|merge_from|
//! reparent_to_new|detach_script|extend_script` across `editor_server.rs` found
//! zero.
//!
//! # ⚠️ Which clauses are red at HEAD, and which are not
//!
//! Stated here rather than inferred from a red-check verdict:
//!
//! * `detach_script`, `extend_script`, `reparent_to_new` and `make_root` all
//!   **404 at HEAD** — those four functional clauses are load-bearing.
//! * **Save Branch's SERVER half already worked** at HEAD: the route serializes
//!   the subtree, writes the `.tscn`, and marks the node with
//!   `_instance_source`. So the functional Save Branch assertions below would
//!   pass without this change — what was missing, and what the static guard
//!   covers, is the *client* caller.
//!
//! That is why the static-wiring test matters: `EDITOR_HTML` referenced none of
//! these routes at HEAD, so the menu-entry assertions fail there.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdeditor::editor_ui::EDITOR_HTML;
use gdscene::node::Node;
use gdscene::SceneTree;
use gdvariant::Variant;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

// ---------------------------------------------------------------------------
// (1) Static wiring: the served editor actually calls these routes.
// ---------------------------------------------------------------------------

/// The context menu and its handlers reference every branch operation.
///
/// 🔑 This is the half that proves the *client* gap the bead names. The server
/// route for Save Branch worked all along; `EDITOR_HTML` never mentioned it.
#[test]
fn editor_html_wires_the_scene_dock_branch_ops() {
    for route in [
        "/api/node/save_branch",
        "/api/node/detach_script",
        "/api/node/extend_script",
        "/api/node/reparent_to_new",
        "/api/node/make_root",
    ] {
        assert!(
            EDITOR_HTML.contains(route),
            "EDITOR_HTML must call {route}; the scene dock had no client caller for it"
        );
    }
    for action in [
        "save-branch",
        "detach-script",
        "extend-script",
        "reparent-to-new",
        "make-root",
    ] {
        assert!(
            EDITOR_HTML.contains(action),
            "the scene-tree context menu must offer a `{action}` item"
        );
    }
}

// ---------------------------------------------------------------------------
// (2) Functional: each operation through the real HTTP surface.
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

/// **The acceptance test.**
#[test]
fn scene_dock_branch_ops_roundtrip() {
    let port = free_port();

    // A unique project dir for the two operations that write files. `res://`
    // resolves against `project_root` for the script routes, so pointing it at
    // a temp dir keeps the repo clean.
    let project = std::env::temp_dir().join(format!("patina_mgixk110_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(&project).expect("create project dir");

    // tree root -> Main -> { Enemy -> Sprite, A, B, Scripted, Extender, Promote }
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let main = tree
        .add_child(tree_root, Node::new("Main", "Node2D"))
        .unwrap();
    let enemy = tree.add_child(main, Node::new("Enemy", "Node2D")).unwrap();
    tree.add_child(enemy, Node::new("Sprite", "Sprite2D"))
        .unwrap();
    let a = tree.add_child(main, Node::new("A", "Node2D")).unwrap();
    let b = tree.add_child(main, Node::new("B", "Node2D")).unwrap();
    let scripted = tree
        .add_child(main, Node::new("Scripted", "Node2D"))
        .unwrap();
    let extender = tree
        .add_child(main, Node::new("Extender", "Node2D"))
        .unwrap();
    let promote = tree
        .add_child(main, Node::new("Promote", "Node2D"))
        .unwrap();

    tree.get_node_mut(scripted)
        .unwrap()
        .set_property("_script_path", Variant::String("res://old.gd".into()));
    tree.get_node_mut(extender)
        .unwrap()
        .set_property("_script_path", Variant::String("res://base.gd".into()));

    let mut state = EditorState::new(tree);
    state.project_root = Some(project.to_string_lossy().to_string());
    let handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));

    // -- 1. Detach Script, and undo restores it -------------------------
    let resp = http_post(
        port,
        "/api/node/detach_script",
        &format!(r#"{{"node_id":{}}}"#, scripted.raw()),
    );
    assert!(
        resp.contains("200 OK"),
        "detach_script should 200, got:\n{resp}"
    );
    {
        let g = handle.state().lock().unwrap();
        assert_eq!(
            g.scene_tree
                .get_node(scripted)
                .map(|n| n.get_property("_script_path")),
            Some(Variant::Nil),
            "Detach Script must clear the node's script"
        );
    }

    // 🔑 The undo half, through the real undo route — this is what the bead
    // means by "Ctrl+Z restores it".
    let resp = http_post(port, "/api/undo", "{}");
    assert!(resp.contains("200 OK"), "undo should 200, got:\n{resp}");
    {
        let g = handle.state().lock().unwrap();
        assert_eq!(
            g.scene_tree
                .get_node(scripted)
                .map(|n| n.get_property("_script_path")),
            Some(Variant::String("res://old.gd".into())),
            "undo must restore the previous script path exactly"
        );
    }

    // -- 2. Extend Script writes a script that inherits the old one ------
    let resp = http_post(
        port,
        "/api/node/extend_script",
        &format!(
            r#"{{"node_id":{},"path":"res://extended.gd"}}"#,
            extender.raw()
        ),
    );
    assert!(
        resp.contains("200 OK"),
        "extend_script should 200, got:\n{resp}"
    );
    let extended = std::fs::read_to_string(project.join("extended.gd"))
        .expect("extend_script must create the script file");
    // ⚠️ The FIRST line is the operation's whole point, and it must name the
    // node's previous script — not the node's class, which is what a
    // create-script template would write.
    assert_eq!(
        extended.lines().next(),
        Some("extends \"res://base.gd\""),
        "the new script's first line must inherit the old path; got:\n{extended}"
    );
    {
        let g = handle.state().lock().unwrap();
        assert_eq!(
            g.scene_tree
                .get_node(extender)
                .map(|n| n.get_property("_script_path")),
            Some(Variant::String("res://extended.gd".into())),
            "Extend Script must also attach the new script"
        );
    }

    // -- 3. Reparent to New Node wraps the selection ---------------------
    let resp = http_post(
        port,
        "/api/node/reparent_to_new",
        &format!(
            r#"{{"node_ids":[{},{}],"class_name":"Node2D"}}"#,
            a.raw(),
            b.raw()
        ),
    );
    assert!(
        resp.contains("200 OK"),
        "reparent_to_new should 200, got:\n{resp}"
    );
    {
        let g = handle.state().lock().unwrap();
        let a_parent = g.scene_tree.get_node(a).and_then(|n| n.parent());
        let b_parent = g.scene_tree.get_node(b).and_then(|n| n.parent());
        assert_eq!(
            a_parent, b_parent,
            "both selected nodes must end up under the SAME new wrapper"
        );
        let wrapper = a_parent.expect("A must have a parent");
        assert_ne!(
            wrapper, main,
            "the wrapper must be a NEW node, not the original parent — equality \
             here means the nodes were never moved"
        );
        let w = g.scene_tree.get_node(wrapper).expect("wrapper exists");
        assert_eq!(w.class_name(), "Node2D", "the wrapper is a Node2D");
        assert_eq!(
            w.parent(),
            Some(main),
            "the wrapper takes the selection's place under the common parent"
        );
    }

    // -- 4. Save Branch writes the .tscn and leaves an instance ----------
    //
    // ⚠️ This route already worked at HEAD; the gap was the client caller,
    // which the static test above covers. Asserted anyway so a regression in
    // the server half is caught here too.
    //
    // The path is `res://` but this route resolves against the process CWD
    // rather than `project_root`, so it is written under a unique subdirectory
    // and removed at the end.
    let branch_dir = format!("patina_mgixk110_branch_{}", std::process::id());
    let resp = http_post(
        port,
        "/api/node/save_branch",
        &format!(
            r#"{{"node_id":{},"path":"res://{branch_dir}/enemy.tscn"}}"#,
            enemy.raw()
        ),
    );
    assert!(
        resp.contains("200 OK"),
        "save_branch should 200, got:\n{resp}"
    );
    let branch_path = std::env::current_dir()
        .unwrap_or_default()
        .join(&branch_dir)
        .join("enemy.tscn");
    let written = std::fs::read_to_string(&branch_path);
    {
        let g = handle.state().lock().unwrap();
        let node = g.scene_tree.get_node(enemy).expect("Enemy still exists");
        assert_eq!(
            node.get_property("_instance_source"),
            Variant::String(format!("res://{branch_dir}/enemy.tscn")),
            "Enemy must now report as an instance of the saved file"
        );
        assert!(
            node.children().is_empty(),
            "the branch's children move into the saved scene, so Enemy is left \
             as a bare instance"
        );
    }
    let _ = std::fs::remove_dir_all(
        std::env::current_dir()
            .unwrap_or_default()
            .join(&branch_dir),
    );
    let written = written.expect("save_branch must write the .tscn");
    assert!(
        written.contains("Enemy") || written.contains("Sprite"),
        "the written scene must contain the branch; got:\n{written}"
    );

    // -- 5. Make Scene Root — done LAST, it restructures the tree ---------
    let resp = http_post(
        port,
        "/api/node/make_root",
        &format!(r#"{{"node_id":{}}}"#, promote.raw()),
    );
    assert!(
        resp.contains("200 OK"),
        "make_root should 200, got:\n{resp}"
    );
    {
        let g = handle.state().lock().unwrap();
        // 🔑 Asserted as "first child of the tree root" because that is exactly
        // how both readers resolve the scene root — `scene_root_id` and, more
        // importantly, the SAVE path, which resolves it inline the same way. So
        // this assertion is what the bead's "the saved file has it as the root"
        // reduces to, without writing a second file.
        let first = g
            .scene_tree
            .get_node(g.scene_tree.root_id())
            .and_then(|n| n.children().first().copied());
        assert_eq!(
            first,
            Some(promote),
            "the promoted node must be the tree root's first child — that is \
             what the save path serializes as the scene root"
        );
        assert_eq!(
            g.scene_tree.get_node(main).and_then(|n| n.parent()),
            Some(promote),
            "the old root must become a child of the new one"
        );
    }

    handle.stop();
    let _ = std::fs::remove_dir_all(&project);
}
