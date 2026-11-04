//! Godot's Play (F5) runs `application/run/main_scene`; Play Current Scene
//! (F6) runs the scene open in the editor. Patina's two buttons sent the SAME
//! request, so Play on the platformer demo with `level/level.tscn` open ran a
//! level with no player at all — `game_singleplayer.tscn` (the main scene) is
//! what instances `Player` under the level.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn request(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

fn runtime_paths(port: u16) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(&request(port, "GET", "/api/runtime/scene", "")).expect("json");
    let mut out = Vec::new();
    fn walk(n: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(p) = n["path"].as_str() {
            out.push(p.to_string());
        }
        for c in n["children"].as_array().into_iter().flatten() {
            walk(c, out);
        }
    }
    walk(&v["tree"], &mut out);
    out
}

#[test]
fn play_runs_the_projects_main_scene_and_play_current_runs_the_open_scene() {
    gdobject::class_db::register_all_classes();
    let project = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer")
        .canonicalize()
        .expect("games/platformer fixture");
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let handle = EditorServerHandle::start(port, EditorState::new(SceneTree::new()));
    std::thread::sleep(Duration::from_millis(100));
    request(port, "POST", "/api/project/open", &format!(r#"{{"path":"{}"}}"#, project.display()));
    let load = request(
        port,
        "POST",
        "/api/scene/load",
        &format!(r#"{{"path":"{}"}}"#, project.join("level/level.tscn").display()),
    );
    assert!(load.contains("\"ok\""), "load: {load}");

    // Play (F5): the main scene, which puts the Player inside the level.
    let r = request(port, "POST", "/api/runtime/play", r#"{"scene":"main"}"#);
    assert!(r.contains("\"ok\":true"), "play: {r}");
    let main = runtime_paths(port);
    request(port, "POST", "/api/runtime/stop", "");
    assert!(
        main.iter().any(|p| p.ends_with("/Game/Level/Player")),
        "Play must run run/main_scene (game_singleplayer.tscn): {:?}",
        main.iter().take(8).collect::<Vec<_>>()
    );

    // The edited scene is untouched by the swap.
    {
        let state = handle.state();
        let s = state.lock().unwrap();
        let root = s.scene_tree.root_id();
        let top: Vec<String> = s.scene_tree.get_node(root).unwrap().children().iter()
            .map(|&c| s.scene_tree.get_node(c).unwrap().name().to_string()).collect();
        assert_eq!(top, vec!["Level".to_string()], "the editor must still hold level.tscn");
        assert!(s.scene_file.as_deref().is_some_and(|f| f.ends_with("level/level.tscn")));
    }

    // Play Current Scene (F6): the open level, which has no player of its own.
    let r = request(port, "POST", "/api/runtime/play", "{}");
    assert!(r.contains("\"ok\":true"), "play current: {r}");
    let current = runtime_paths(port);
    request(port, "POST", "/api/runtime/stop", "");
    assert!(current.iter().any(|p| p == "/root/Level"), "{:?}", current.iter().take(8).collect::<Vec<_>>());
    assert!(!current.iter().any(|p| p.ends_with("/Player")), "level.tscn alone has no Player");
    handle.stop();
}
