//! pat-nyn3w: the renderer builds an inline Environment from the node
//! companions (the store scripts write through) instead of re-reading the
//! scene FILE. For a scene no script touched, both must give the same
//! Environment, so the switch is behaviour-neutral for platformer_3d.
//!
//! ⚠️ Its own test binary: it installs the platformer_3d `res://` root, a
//! process-global, which would race the truck_town tests' root if they shared
//! a process under plain `cargo test`.

use gdscene::packed_scene::PackedScene;
use gdvariant::Variant;

/// For a scene no script has touched, the companion-built environment is the
/// one the scene FILE describes: platformer_3d's `stage.tscn` inline
/// Environment -> Sky -> ShaderMaterial (an `ExtResource` shader and cubemap)
/// resolves identically both ways, so switching the renderer to the
/// companions changes nothing for it.
#[test]
fn an_untouched_inline_environment_matches_the_scene_file() {
    gdobject::class_db::register_all_classes();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/platformer_3d")
        .canonicalize()
        .unwrap();
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let tscn = std::fs::read_to_string(dir.join("stage/stage.tscn")).unwrap();
    let packed = PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = gdscene::SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();
    let from_nodes = gdscene::world_environment::runtime_environment_3d(&tree, None)
        .expect("stage.tscn authors an environment");
    let from_file = gdscene::world_environment::environment_from_scene_file(
        "res://stage/stage.tscn",
        "Environment_vpofs",
    )
    .unwrap();
    assert!(from_nodes.sky.is_some(), "the stage has a sky");
    assert_eq!(from_nodes, from_file);
    // ... and the node-built path is the one taken: the nested Sky ->
    // ShaderMaterial body is on the WorldEnvironment node.
    let we = tree
        .get_node_by_path("/root/Stage/WorldEnvironment")
        .unwrap();
    assert_eq!(
        tree.get_node(we)
            .unwrap()
            .get_property("__subres__environment:sky:sky_material__shader"),
        Variant::String("ExtResource:3_s88my".into())
    );
}
