//! A CharacterBody2D's CollisionShape2D offset is part of where it collides.
//!
//! The character solver placed the shape at the body's origin, dropping the
//! collider's local transform. The official 2D Platformer demo's player has
//! its shape at `(0, -14)`, so it stood 14 px above the floor — and was
//! popped 13.75 px up out of the floor it was authored resting on.

use gdcore::math::Vector2;
use gdscene::main_loop::MainLoop;
use gdscene::node::Node;
use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

#[test]
fn a_character_rests_with_its_offset_shape_on_the_floor() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Floor: top edge at y = 20.
    let mut floor = Node::new("Floor", "StaticBody2D");
    floor.set_property("position", Variant::Vector2(Vector2::new(0.0, 30.0)));
    let floor_id = tree.add_child(root, floor).unwrap();
    let mut fs = Node::new("Shape", "CollisionShape2D");
    fs.set_property("shape", Variant::String("RectangleShape2D".into()));
    fs.set_property("size", Variant::Vector2(Vector2::new(400.0, 20.0)));
    tree.add_child(floor_id, fs).unwrap();

    // Character: a 10x10 box offset 14 px UP inside the body.
    let mut body = Node::new("Player", "CharacterBody2D");
    body.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let body_id = tree.add_child(root, body).unwrap();
    let mut cs = Node::new("Shape", "CollisionShape2D");
    cs.set_property("shape", Variant::String("RectangleShape2D".into()));
    cs.set_property("size", Variant::Vector2(Vector2::new(10.0, 10.0)));
    cs.set_property("position", Variant::Vector2(Vector2::new(0.0, -14.0)));
    tree.add_child(body_id, cs).unwrap();
    let src = "extends CharacterBody2D\nfunc _physics_process(delta):\n\
\tvelocity += get_gravity() * delta\n\tmove_and_slide()\n";
    tree.attach_script(
        body_id,
        Box::new(GDScriptNodeInstance::from_source(src, body_id).unwrap()),
    );
    gdscene::LifecycleManager::enter_tree(&mut tree, body_id);

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();

    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }
    let y = match ml
        .tree()
        .get_node(body_id)
        .unwrap()
        .get_property("position")
    {
        Variant::Vector2(v) => v.y,
        other => panic!("{other:?}"),
    };
    // Shape bottom = y - 14 + 5 must meet the floor top at 20, so y = 29.
    // Ignoring the offset rests the BODY ORIGIN's shape at 20, i.e. y = 15.
    assert!(
        (y - 29.0).abs() < 1.0,
        "the offset shape rests on the floor (y = 29), got y = {y}"
    );
}
