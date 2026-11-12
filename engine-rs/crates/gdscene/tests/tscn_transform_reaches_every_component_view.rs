//! A `.tscn` node that authors only `transform` reports it through every view.
//!
//! Godot stores one `local_transform`; `position`/`rotation`/`scale` are views
//! of it. Patina keeps separate keys and ClassDB-seeds `position = ZERO` before
//! the scene's values apply, so a node authored at (5,6,7) used to answer
//! `get_position()` with ZERO while `get_local_transform()` said (5,6,7).

use gdcore::math::Vector3;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Mesh" type="Node3D" parent="."]
transform = Transform3D(2, 0, 0, 0, 2, 0, 0, 0, 2, 5, 6, 7)
"#;

#[test]
fn an_authored_transform_reaches_position_and_scale() {
    // Seeding is what made the views stale, so it must be on.
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instances");
    let mesh = tree.find_child(main, "Mesh", true).expect("Mesh");

    let p = gdscene::node3d::get_position(&tree, mesh);
    assert!(
        (p - Vector3::new(5.0, 6.0, 7.0)).length() < 1e-4,
        "position view: {p:?}"
    );
    match tree.get_node(mesh).unwrap().get_property("scale") {
        gdvariant::Variant::Vector3(s) => {
            assert!(
                (s - Vector3::new(2.0, 2.0, 2.0)).length() < 1e-4,
                "scale view: {s:?}"
            )
        }
        other => panic!("scale must be a Vector3, got {other:?}"),
    }
}
