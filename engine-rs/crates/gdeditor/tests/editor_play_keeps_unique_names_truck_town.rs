//! Editor Play resolves `%UniqueName` paths, as the runner and Godot do.
//!
//! truck_town's `car_select.gd` (games/truck_town, the official Godot demo
//! `3d/truck_town`, unmodified) reads every widget through `%Name`
//! (`@onready var car_container: HBoxContainer = %CarContainer`). Play clones
//! the edited tree with `clone_scene_tree`, which copied each node's
//! properties and groups but NOT the Node's unique-name flag, so in the editor
//! every `%Name` read null and `_ready` died on its first line ("cannot call
//! method on Nil") while the headless runner booted the same scene cleanly.
//! Repro: play `car_select/car_select.tscn` in the editor
//! (`scripts/patina-mcp play '{"frames": 5}'`).

use gdeditor::editor_server::{build_play_tree, EditorState};
use gdscene::packed_scene::PackedScene;
use gdscene::SceneTree;
use gdvariant::Variant;

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/truck_town")
        .canonicalize()
        .expect("games/truck_town fixture")
}

#[test]
fn play_resolves_unique_names_in_car_select() {
    gdobject::class_db::register_all_classes();
    let dir = game_dir();
    gdcore::set_path_roots(Some(dir.clone()), None);
    let text = std::fs::read_to_string(dir.join("car_select/car_select.tscn")).unwrap();
    let scene = PackedScene::from_tscn(&text).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree_for_editing(&mut tree, root, &scene)
        .expect("car_select.tscn opens for editing");

    let mut state = EditorState::new(tree);
    state
        .texture_cache
        .set_project_root(dir.to_string_lossy().to_string());
    state.scene_file = Some("car_select/car_select.tscn".to_string());

    let mut played = build_play_tree(&mut state, None);

    let car_select = played
        .get_node_by_path("/root/CarSelect")
        .expect("CarSelect in the played tree");
    let car_container = played
        .get_node_by_path("/root/CarSelect/CarContainer")
        .expect("CarContainer in the played tree");
    assert_eq!(
        played.get_node_relative(car_select, "%CarContainer"),
        Some(car_container),
        "%CarContainer must resolve in the played tree"
    );
    let script = played
        .get_script(car_select)
        .expect("car_select.gd attached");
    assert_eq!(
        script.get_property("car_container"),
        Some(Variant::ObjectId(car_container.object_id())),
        "@onready var car_container = %CarContainer must bind the HBoxContainer"
    );
    let out = played.drain_script_output();
    assert!(
        out.errors.is_empty(),
        "car_select.gd _ready must run without script errors in Play, got {:?}",
        out.errors
    );
}
