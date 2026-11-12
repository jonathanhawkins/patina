//! pat-7puwj: truck_town's car select must load the town under editor Play.
//!
//! Picking a car runs `car_select.gd::_load_scene`, whose last steps are
//!
//! ```gdscript
//! town.setup(car, _on_back_pressed, button_sdfgi.button_pressed)
//! get_parent().add_child(town)
//! ```
//!
//! and `town_scene.gd::setup` ends with
//! `%WorldEnvironment.environment.sdfgi_enabled = sdfgi`. Two bugs stopped it,
//! each aborting `_load_scene` before the town was added:
//!
//! 1. editor Play read the never-authored `button_sdfgi.button_pressed` as null
//!    (the editor binary loaded its launch scene before ClassDB was populated, so
//!    no node carried its class defaults) — "setup(): expected element of type
//!    bool, got Nil";
//! 2. then, in the runner too, the write into the WorldEnvironment's inline
//!    `Environment` sub-resource — "cannot assign member 'sdfgi_enabled' on
//!    String".
//!
//! This drives the editor binary's own launch path (`load_launch_scene`, which
//! must not be preceded by a ClassDB registration here — so this file holds ONE
//! test) and editor Play's tree (`build_play_tree`), presses the Mini Van the way
//! its `pressed` connection does, and steps frames until the town is in.

use gdeditor::editor_server::{
    apply_project_path_roots, build_play_tree, load_launch_scene, EditorState,
};
use gdscene::MainLoop;
use gdvariant::Variant;
use std::path::Path;

#[test]
fn picking_a_car_loads_the_town_in_editor_play() {
    let game = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../games/truck_town")
        .canonicalize()
        .unwrap();
    let root = game.to_string_lossy().to_string();
    apply_project_path_roots(&root);
    let scene = game.join("car_select/car_select.tscn");
    let (tree, _scene_root, _source) = load_launch_scene(&scene.to_string_lossy()).expect("opens");

    // The editor binary's state setup (examples/editor.rs).
    let mut state = EditorState::new(tree);
    state.scene_file = Some(scene.to_string_lossy().to_string());
    state.texture_cache = gdscene::texture_cache::TextureCache::new(&root);
    state.project_root = Some(root.clone());
    let play = build_play_tree(&mut state, None);
    let mut ml = MainLoop::new(play);
    ml.step(1.0 / 60.0);
    let _ = ml.tree_mut().drain_script_errors();

    let car_select = ml
        .tree()
        .get_node_by_path("/root/CarSelect")
        .expect("/root/CarSelect");
    ml.tree_mut()
        .call_script_method(car_select, "_on_mini_van_pressed", &[]);
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }
    let errors = ml.tree_mut().drain_script_errors();
    assert!(
        errors.iter().all(|e| !e.contains("setup()")),
        "town.setup() must run to its end: {errors:#?}"
    );
    let town = ml
        .tree()
        .get_node_by_path("/root/TownScene")
        .unwrap_or_else(|| panic!("the town was never added to /root; script errors: {errors:#?}"));
    assert!(
        ml.tree()
            .get_node_by_path("/root/TownScene/InstancePos/car")
            .is_some(),
        "setup() puts the picked car under %InstancePos"
    );
    // `button_sdfgi.button_pressed` is unset in car_select.tscn, so Godot passes
    // its class default `false` — and the write lands on the inline Environment.
    let env = ml
        .tree()
        .get_node_relative(town, "WorldEnvironment")
        .expect("WorldEnvironment");
    assert_eq!(
        ml.tree()
            .get_node(env)
            .unwrap()
            .get_property("__subres__environment__sdfgi_enabled"),
        Variant::Bool(false),
        "`environment.sdfgi_enabled = sdfgi` must land on the WorldEnvironment's Environment"
    );
}
