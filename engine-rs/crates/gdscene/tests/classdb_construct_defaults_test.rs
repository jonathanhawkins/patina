//! pat-nzgdu.100 — constructing a node via `SceneTree::create_node` applies the
//! class's registered ClassDB property defaults up the inheritance chain, so a
//! freshly created Node2D already has `scale == (1,1)`, `visible == true`,
//! `position == (0,0)`, `rotation == 0` before anything sets them — reading an
//! unset registered property yields the Godot class default, not `Nil`.

use gdcore::math::Vector2;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

#[test]
fn classdb_construct_defaults_test() {
    // The runner populates ClassDB at boot; a bare test does it explicitly.
    gdobject::class_db::register_2d_classes();

    let mut tree = SceneTree::new();
    let id = tree.create_node("Node2D", "s");
    let node = tree.get_node(id).expect("created node is in the tree");

    // Node2D's own registered defaults.
    assert_eq!(
        node.get_property("scale"),
        Variant::Vector2(Vector2::ONE),
        "scale defaults to (1,1) from ClassDB, though never set",
    );
    assert_eq!(
        node.get_property("position"),
        Variant::Vector2(Vector2::ZERO),
        "position defaults to (0,0)",
    );
    assert_eq!(node.get_property("rotation"), Variant::Float(0.0));

    // Inherited default from CanvasItem (Node2D → CanvasItem).
    assert_eq!(
        node.get_property("visible"),
        Variant::Bool(true),
        "visible defaults to true from the inherited CanvasItem registration",
    );
}
