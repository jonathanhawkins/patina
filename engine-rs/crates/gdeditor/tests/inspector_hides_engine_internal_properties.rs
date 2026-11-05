//! The inspector lists a node's Godot properties, never the engine's own
//! companions. Found by scripts/editor-tour.mjs on platformer_3d: the
//! instanced TouchScreenUI showed "Instance: ExtResource("5_7jktm")",
//! "Instance Expanded: On", "Instance Path: res://touch_screen_ui/..." and
//! "Extuid Script: uid://..." as editable rows. Godot's inspector has no such
//! properties; the saver already keeps every one of them out of the file.

use gdeditor::editor_server::{load_launch_scene, EditorServerHandle, EditorState};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn get(port: u16, path: &str) -> serde_json::Value {
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
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes())
        .unwrap();
    let mut resp = Vec::new();
    let _ = s.read_to_end(&mut resp);
    let resp = String::from_utf8_lossy(&resp).to_string();
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or(""))
        .unwrap_or_else(|e| panic!("{e}: {resp}"))
}

#[test]
fn an_instanced_nodes_inspector_shows_no_engine_companions() {
    let game = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .unwrap();
    gdcore::set_path_roots(Some(game.clone()), None);
    let (tree, root, _) = load_launch_scene(game.join("game.tscn").to_str().unwrap()).unwrap();
    let ui = tree
        .get_node_relative(root, "TouchScreenUI")
        .expect("TouchScreenUI");
    let coin = tree.get_node_relative(root, "Coins/Coin1").expect("Coin1");
    let mut state = EditorState::new(tree);
    state.project_root = Some(game.to_string_lossy().to_string());
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let handle = EditorServerHandle::start(port, state);

    for (what, id) in [("TouchScreenUI", ui), ("Coin1", coin)] {
        let node = get(port, &format!("/api/node/{}", id.raw()));
        let names: Vec<String> = node["properties"]
            .as_array()
            .unwrap_or_else(|| panic!("{what}: {node}"))
            .iter()
            .filter_map(|p| p["name"].as_str().map(str::to_string))
            .collect();
        let leaked: Vec<&String> = names
            .iter()
            .filter(|n| {
                (gdscene::scene_saver::is_internal_property(n) && !n.starts_with("__"))
                    || n.starts_with("__extuid__")
            })
            .collect();
        assert!(
            leaked.is_empty(),
            "{what}'s inspector lists engine companions: {leaked:?}"
        );
        for gone in [
            "_instance",
            "_instance_path",
            "_instance_expanded",
            "__extuid__script",
        ] {
            assert!(!names.iter().any(|n| n == gone), "{what}: {gone}");
        }
        // ...and its real properties are still there.
        assert!(
            names.iter().any(|n| n == "script"),
            "{what}'s script row: {names:?}"
        );
    }
    handle.stop();
}
