//! The editor opens, shows and saves the official Godot 3D platformer's scenes
//! that instance other scenes (pat-hivyf, pat-rbyty).
//!
//! `player.tscn` and `enemy.tscn` instance a `.glb` with Editable Children:
//! their override entries (`[node name="Robot" parent="Player/Skeleton/Skeleton3D"
//! parent_id_path=…]`) address nodes that exist only inside the glTF. The
//! editor built its edited tree with every instance as a childless `Node`, so
//! both scenes failed to open ("parent path not found"), and `game.tscn` showed
//! Stage, Player and every coin as a classless, childless `Node` with an empty
//! `instance_path` — in both JSON trees.
//!
//! All fixtures are the real, unmodified files under `games/platformer_3d`;
//! every save goes to a temp file.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

fn game_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .unwrap()
}

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(120))).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let status = out
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn json_of(port: u16, method: &str, path: &str, body: &str) -> Value {
    let (status, text) = http(port, method, path, body);
    assert_eq!(status, 200, "{method} {path}: {text}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}: {text}"))
}

/// An editor with the platformer project open and `scene` (relative to the
/// game) loaded.
fn open(scene: &str) -> EditorServerHandle {
    gdobject::class_db::register_all_classes();
    let handle = EditorServerHandle::start(0, EditorState::new(SceneTree::new()));
    let port = handle.port();
    json_of(
        port,
        "POST",
        "/api/project/open",
        &json!({"path": game_dir()}).to_string(),
    );
    let (status, body) = http(
        port,
        "POST",
        "/api/scene/load",
        &json!({"path": game_dir().join(scene)}).to_string(),
    );
    assert_eq!(status, 200, "opening {scene} failed: {body}");
    handle
}

/// The edit tree's node at a `/`-separated name path below the scene root.
fn find<'a>(tree: &'a Value, path: &str) -> &'a Value {
    let mut node = &tree["children"][0];
    for name in path.split('/').filter(|s| !s.is_empty()) {
        node = node["children"]
            .as_array()
            .and_then(|c| c.iter().find(|n| n["name"] == name))
            .unwrap_or_else(|| panic!("no `{name}` on the way to `{path}` in the edit tree"));
    }
    node
}

fn edit_tree(port: u16) -> Value {
    json_of(port, "GET", "/api/scene", "")["nodes"].clone()
}

fn node_id(handle: &EditorServerHandle, path: &str) -> u64 {
    let state = handle.state();
    let s = state.lock().unwrap();
    let root = s
        .scene_tree
        .get_node(s.scene_tree.root_id())
        .unwrap()
        .children()[0];
    let id = if path == "." {
        Some(root)
    } else {
        s.scene_tree.get_node_relative(root, path)
    };
    id.unwrap_or_else(|| panic!("no node {path}")).raw()
}

fn node_value(handle: &EditorServerHandle, path: &str, prop: &str) -> Variant {
    let state = handle.state();
    let s = state.lock().unwrap();
    let root = s
        .scene_tree
        .get_node(s.scene_tree.root_id())
        .unwrap()
        .children()[0];
    let id = s
        .scene_tree
        .get_node_relative(root, path)
        .unwrap_or_else(|| panic!("no node {path}"));
    s.scene_tree.get_node(id).unwrap().get_property(prop)
}

fn save_to_temp(port: u16, dir: &tempfile::TempDir, name: &str) -> String {
    let out = dir.path().join(name);
    let (status, body) = http(
        port,
        "POST",
        "/api/scene/save",
        &json!({"path": out}).to_string(),
    );
    assert_eq!(status, 200, "save: {body}");
    std::fs::read_to_string(out).unwrap()
}

#[test]
fn player_and_enemy_open_with_the_glb_expanded_and_its_editable_children_shown() {
    for (scene, inst, glb, foreign_mesh, own_child, override_value) in [
        (
            "player/player.tscn",
            "Player",
            "res://player/player.glb",
            "Player/Skeleton/Skeleton3D/Robot",
            "Player/Skeleton/Bullet",
            Some(("layers", Variant::Int(2))),
        ),
        (
            "enemy/enemy.tscn",
            "Enemy",
            "res://enemy/enemy.glb",
            "Enemy/Skeleton/Skeleton3D/Body",
            "Enemy/Skeleton/RayFloor",
            None,
        ),
    ] {
        let handle = open(scene);
        let tree = edit_tree(handle.port());

        let instance = find(&tree, inst);
        assert_eq!(
            instance["class"], "Node3D",
            "{scene}: the instance root keeps the glTF root's class"
        );
        assert_eq!(instance["is_instance"], true, "{scene}");
        assert_eq!(instance["instance_path"], glb, "{scene}");
        assert_eq!(
            instance["editable_children"], true,
            "{scene}: `[editable path=\"{inst}\"]`"
        );

        // The glTF's own mesh, inside the editable instance: shown, and foreign.
        let mesh = find(&tree, foreign_mesh);
        assert_eq!(mesh["class"], "MeshInstance3D", "{scene}");
        assert_eq!(
            mesh["foreign"], true,
            "{scene}: {foreign_mesh} was declared by the glTF"
        );
        // The scene's own node parented INSIDE the glTF: shown, not foreign.
        let own = find(&tree, own_child);
        assert_eq!(
            own["foreign"], false,
            "{scene}: {own_child} is the scene's own"
        );
        if let Some((prop, want)) = override_value {
            assert_eq!(
                node_value(&handle, foreign_mesh, prop),
                want,
                "{scene}: the override entry applied"
            );
        }
        // The 3D view draws the glTF's mesh: it has one to draw.
        assert!(
            !node_value(&handle, foreign_mesh, "mesh").is_nil(),
            "{scene}: {foreign_mesh} carries its glTF mesh"
        );
    }
}

#[test]
fn game_scene_instances_show_their_real_class_and_source_and_hide_their_internals() {
    let handle = open("game.tscn");
    let tree = edit_tree(handle.port());
    for (path, class, source) in [
        ("Stage", "Node3D", "res://stage/stage.tscn"),
        ("Player", "CharacterBody3D", "res://player/player.tscn"),
        ("Coins/Coin1", "Area3D", "res://coin/coin.tscn"),
        ("Enemies/Enemy1", "RigidBody3D", "res://enemy/enemy.tscn"),
    ] {
        let n = find(&tree, path);
        assert_eq!(n["class"], class, "{path}");
        assert_eq!(n["instance_path"], source, "{path}");
        assert_eq!(n["is_instance"], true, "{path}");
        // No Editable Children in game.tscn: Godot's dock shows the instance as
        // one node.
        assert_eq!(
            n["children"].as_array().map(Vec::len),
            Some(0),
            "{path}: {}",
            n["children"]
        );
    }
    // ...while the edited tree really holds the instances' nodes.
    let state = handle.state();
    let s = state.lock().unwrap();
    assert!(
        s.scene_tree.node_count() > 600,
        "game.tscn expanded: {} nodes",
        s.scene_tree.node_count()
    );
}

#[test]
fn every_unedited_real_scene_saves_back_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let mut seen = 0;
    for scene in [
        "game.tscn",
        "player/player.tscn",
        "enemy/enemy.tscn",
        "coin/coin.tscn",
        "stage/stage.tscn",
        "player/bullet/bullet.tscn",
        "touch_screen_ui/touch_screen_ui.tscn",
        "touch_screen_ui/virtual_joystick/virtual_joystick_scene.tscn",
    ] {
        let original = std::fs::read_to_string(game_dir().join(scene)).unwrap();
        let handle = open(scene);
        // Reading the scene must not change it.
        edit_tree(handle.port());
        let saved = save_to_temp(handle.port(), &dir, &scene.replace('/', "_"));
        assert!(
            saved == original,
            "{scene}: an unedited save changed the file:\n{}",
            first_diff(&original, &saved)
        );
        // Saving twice is stable too.
        let again = save_to_temp(handle.port(), &dir, &scene.replace('/', "_"));
        assert!(
            again == original,
            "{scene}: the second save changed the file"
        );
        seen += 1;
    }
    assert_eq!(seen, 8);
}

fn first_diff(a: &str, b: &str) -> String {
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}:\n  file: {x}\n  save: {y}", i + 1);
        }
    }
    format!(
        "lengths {} vs {} lines",
        a.lines().count(),
        b.lines().count()
    )
}

#[test]
fn an_edit_inside_the_glb_is_saved_as_an_override_and_nothing_else_changes() {
    let dir = tempfile::tempdir().unwrap();

    // An existing editable-children override, changed.
    let original = std::fs::read_to_string(game_dir().join("player/player.tscn")).unwrap();
    let handle = open("player/player.tscn");
    let robot = node_id(&handle, "Player/Skeleton/Skeleton3D/Robot");
    json_of(
        handle.port(),
        "POST",
        "/api/property/set",
        &json!({"node_id": robot, "property": "layers", "value": {"type": "Int", "value": 4}})
            .to_string(),
    );
    let saved = save_to_temp(handle.port(), &dir, "player.tscn");
    assert_eq!(
        saved,
        original.replace(
            "index=\"0\" unique_id=868948960]\nlayers = 2\n",
            "index=\"0\" unique_id=868948960]\nlayers = 4\n"
        ),
        "only the edited line changes; parent_id_path, unique_id and the rest stay"
    );
    let reread = PackedScene::from_tscn(&saved).unwrap().canonical_form();
    let expected = PackedScene::from_tscn(&original)
        .unwrap()
        .canonical_form()
        .replace("  layers = Int(2)", "  layers = Int(4)");
    assert_eq!(
        reread, expected,
        "the saved scene says the same as the file, plus the edit"
    );

    // A glTF node the file never overrode, edited for the first time.
    let original = std::fs::read_to_string(game_dir().join("enemy/enemy.tscn")).unwrap();
    let handle = open("enemy/enemy.tscn");
    let body = node_id(&handle, "Enemy/Skeleton/Skeleton3D/Body");
    json_of(
        handle.port(),
        "POST",
        "/api/property/set",
        &json!({"node_id": body, "property": "layers", "value": {"type": "Int", "value": 8}})
            .to_string(),
    );
    let saved = save_to_temp(handle.port(), &dir, "enemy.tscn");
    assert!(
        saved.contains("[node name=\"Body\" parent=\"Enemy/Skeleton/Skeleton3D\"]\nlayers = 8\n"),
        "a typeless override entry carrying just the edit:\n{saved}"
    );
    let added: Vec<&str> = saved
        .lines()
        .filter(|l| !original.lines().any(|o| o == *l))
        .collect();
    assert_eq!(
        added,
        [
            "[node name=\"Body\" parent=\"Enemy/Skeleton/Skeleton3D\"]",
            "layers = 8"
        ],
        "nothing else is added"
    );
    assert_eq!(
        original
            .lines()
            .filter(|l| !saved.lines().any(|s| s == *l))
            .count(),
        0,
        "nothing of the file is lost"
    );

    // The saved text opens again with the edit applied and the glTF intact.
    // Opened in-process, rooted at the game, so nothing is written into it.
    gdcore::set_path_roots(Some(game_dir()), None);
    let scene = PackedScene::from_tscn(&saved).unwrap();
    let mut tree = SceneTree::new();
    let r = tree.root_id();
    let root = gdscene::packed_scene::add_packed_scene_to_tree_for_editing(&mut tree, r, &scene)
        .expect("the saved enemy scene re-opens");
    let at = |path: &str, prop: &str| {
        let id = tree
            .get_node_relative(root, path)
            .unwrap_or_else(|| panic!("no {path}"));
        tree.get_node(id).unwrap().get_property(prop)
    };
    let (layers, ray) = (
        at("Enemy/Skeleton/Skeleton3D/Body", "layers"),
        at("Enemy/Skeleton/RayFloor", "target_position"),
    );
    assert_eq!(layers, Variant::Int(8));
    assert!(
        !ray.is_nil(),
        "the scene's own RayFloor under the glTF is still there"
    );
}

#[test]
fn the_running_game_tree_names_each_instance_source_and_play_attaches_instanced_players() {
    let handle = open("game.tscn");
    let port = handle.port();
    let (status, body) = http(port, "POST", "/api/runtime/play", "{}");
    assert_eq!(status, 200, "play: {body}");
    let rt = json_of(port, "GET", "/api/runtime/scene", "");
    let rt_tree = rt.get("tree").cloned().unwrap_or(rt);
    let game = rt_tree["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Game")
        .expect("Game in the running tree")
        .clone();
    let child = |name: &str| {
        game["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == name)
            .cloned()
            .unwrap()
    };
    let player = child("Player");
    assert_eq!(player["class"], "CharacterBody3D");
    assert_eq!(player["instance_path"], "res://player/player.tscn");
    assert_eq!(child("Stage")["instance_path"], "res://stage/stage.tscn");
    // The running tree shows everything, as Godot's remote tree does.
    assert!(!player["children"].as_array().unwrap().is_empty());

    // Each coin's own AnimationPlayer, declared by coin.tscn, runs in Play.
    {
        let state = handle.state();
        let s = state.lock().unwrap();
        let tree = s.run_main_loop.as_ref().expect("running").tree();
        let game_id = tree
            .get_node(tree.root_id())
            .unwrap()
            .children()
            .iter()
            .copied()
            .find(|&id| tree.get_node(id).unwrap().name() == "Game")
            .unwrap();
        let anim = tree
            .get_node_relative(game_id, "Coins/Coin1/Animation")
            .expect("Coin1/Animation");
        let player = tree
            .get_animation_player(anim)
            .expect("coin.tscn's AnimationPlayer is attached in Play");
        assert!(player.has_animation("spin"), "coin.tscn's `spin` clip");
    }
    http(port, "POST", "/api/runtime/stop", "{}");
}

#[test]
fn the_mcp_tree_takes_a_budget_and_says_how_many_nodes_it_left_out() {
    let handle = open("game.tscn");
    let port = handle.port();
    let call = |args: Value| -> String {
        let reply = json_of(
            port,
            "POST",
            "/mcp",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "tree", "arguments": args}}).to_string(),
        );
        reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{reply}"))
            .to_string()
    };
    let total = {
        let state = handle.state();
        let s = state.lock().unwrap();
        let root = s
            .scene_tree
            .get_node(s.scene_tree.root_id())
            .unwrap()
            .children()[0];
        let mut ids = Vec::new();
        s.scene_tree.collect_subtree_top_down(root, &mut ids);
        ids.len()
    };
    assert!(total > 600, "game.tscn has {total} nodes");

    let cut = call(json!({"props": false, "budget": 50}));
    assert!(
        cut.contains(&format!("… {} of {total} nodes omitted", total - 50)),
        "the cut says how much it hid:\n{}",
        cut.lines().last().unwrap_or("")
    );
    let default = call(json!({"props": false}));
    assert!(
        default.contains(&format!("… {} of {total} nodes omitted", total - 400)),
        "default budget 400"
    );

    let all = call(json!({"props": false, "budget": 100000}));
    assert!(
        !all.contains("omitted"),
        "a large enough budget lists everything"
    );
    assert_eq!(all.lines().count(), total, "one line per node");
    assert!(
        all.contains(
            "Player (CharacterBody3D) [res://player/player.gd] <res://player/player.tscn>"
        ),
        "{}",
        &all[..400.min(all.len())]
    );
}
