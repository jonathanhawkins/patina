//! pat-i5igl: instancing a scene from the FileSystem dock builds it the way
//! the editor's Open does — expanded, its nodes foreign to the edited scene —
//! and saves back as one `instance=` line, as Godot does.
//!
//! The dock's `EditorCommand::InstanceScene` used the placeholder builder,
//! which cannot attach an editable child of an instance: the platformer_3d
//! `player.tscn` puts `Bullet` under `Player/Skeleton`, a node that exists only
//! inside player.glb, so instancing it failed with "parent path not found".

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn http(port: u16, path: &str, body: &str) -> serde_json::Value {
    let mut stream = None;
    for _ in 0..40 {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            stream = Some(s);
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let mut s = stream.expect("editor server never accepted");
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    let resp = String::from_utf8_lossy(&resp).to_string();
    let body = resp.split("\r\n\r\n").nth(1).unwrap_or("");
    serde_json::from_str(body).unwrap_or_else(|e| panic!("not JSON ({e}): {resp}"))
}

#[test]
fn the_dock_instances_player_tscn_and_saves_it_as_one_instance_line() {
    gdobject::class_db::register_all_classes();
    let game =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d");
    let game = game.canonicalize().unwrap();

    let mut tree = SceneTree::new();
    let r = tree.root_id();
    let main = tree.add_child(r, Node::new("Main", "Node3D")).unwrap();
    let mut state = EditorState::new(tree);
    state.project_root = Some(game.to_string_lossy().to_string());
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, state);

    let resp = http(
        port,
        "/api/scene/instance",
        &format!(
            r#"{{"path":"res://player/player.tscn","parent_id":{}}}"#,
            main.raw()
        ),
    );
    assert!(
        resp.get("error").is_none(),
        "instancing player.tscn failed: {resp}"
    );

    {
        let st = handle.state().lock().unwrap();
        let t = &st.scene_tree;
        let player = t
            .get_node_relative(main, "Player")
            .expect("the instance root, named as its scene root");
        assert_eq!(t.get_node(player).unwrap().class_name(), "CharacterBody3D");
        // Its nodes are built, glTF included, and belong to the instance.
        let bullet = t
            .get_node_relative(player, "Player/Skeleton/Bullet")
            .expect("player.tscn's editable child");
        assert!(gdscene::packed_scene::is_foreign_node(t, main, bullet));
        assert!(!gdscene::packed_scene::is_foreign_node(t, main, player));
    }

    let out =
        std::env::temp_dir().join(format!("patina_dock_instance_{}.tscn", std::process::id()));
    let saved = http(
        port,
        "/api/scene/save",
        &format!(r#"{{"path":"{}"}}"#, out.display()),
    );
    assert!(saved.get("error").is_none(), "{saved}");
    let text = std::fs::read_to_string(&out).unwrap();
    let _ = std::fs::remove_file(&out);
    let nodes: Vec<&str> = text.lines().filter(|l| l.starts_with("[node ")).collect();
    assert_eq!(
        nodes.len(),
        2,
        "Main and the one instance line, nothing of the instance's own:\n{text}"
    );
    assert!(
        nodes[1].starts_with("[node name=\"Player\" parent=\".\" instance=ExtResource("),
        "{text}"
    );
    assert!(text.contains("path=\"res://player/player.tscn\""), "{text}");
    handle.stop();
}
