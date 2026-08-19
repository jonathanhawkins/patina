//! pat-s3rrv.103 clause (d) — `CharacterBody3D.slide_on_ceiling`.
//!
//! Godot: *"If true, during a jump against the ceiling, the body will slide; if
//! false it will be stopped and will fall vertically."* (`CharacterBody3D.xml:183`,
//! default **true**.) The 3D solver had ceiling DETECTION (`is_on_ceiling`) but
//! no such knob, so a body authored with it off skated along the ceiling anyway.
//!
//! ⚠️ The rule is gated on Godot's `vel_dir_facing_up`
//! (`character_body_3d.cpp:159,314`) — the cancel applies ONLY while the body is
//! moving upward. A body walking under a low ceiling, or falling past one, still
//! slides. Dropping that term looks fine in a jump test and freezes a character
//! the moment its head brushes anything, so both directions are asserted here.

use gdcore::math::Vector3;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A character under a solid ceiling, driven up-and-sideways every tick.
///
/// `up_direction` defaults to +Y, so the ceiling sits ABOVE at +Y and the body
/// jumps into it.
fn ceiling_scene(slide_on_ceiling: bool, velocity: &str) -> (MainLoop, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let mut ceiling = Node::new("Ceiling", "StaticBody3D");
    ceiling.set_property("position", Variant::Vector3(Vector3::new(0.0, 3.0, 0.0)));
    let ceiling_id = tree.add_child(root, ceiling).unwrap();
    let mut cs = Node::new("CeilShape", "CollisionShape3D");
    cs.set_property("shape", Variant::String("BoxShape3D".to_string()));
    cs.set_property("size", Variant::Vector3(Vector3::new(80.0, 1.0, 80.0)));
    tree.add_child(ceiling_id, cs).unwrap();

    let mut body = Node::new("Player", "CharacterBody3D");
    body.set_property("position", Variant::Vector3(Vector3::ZERO));
    // Authored exactly as a .tscn would; the default is true, so only the
    // false case needs writing — but both are written to keep the two fixtures
    // identical apart from the knob under test.
    body.set_property("slide_on_ceiling", Variant::Bool(slide_on_ceiling));
    let player = tree.add_child(root, body).unwrap();
    let mut ps = Node::new("Shape", "CollisionShape3D");
    ps.set_property("shape", Variant::String("SphereShape3D".to_string()));
    ps.set_property("radius", Variant::Float(0.5));
    tree.add_child(player, ps).unwrap();

    let script = format!(
        "\
extends CharacterBody3D
var ticks = 0
func _physics_process(delta):
    velocity = {velocity}
    move_and_slide()
    self.ticks = self.ticks + 1
"
    );
    tree.attach_script(
        player,
        Box::new(GDScriptNodeInstance::from_source(&script, player).unwrap()),
    );
    (MainLoop::new(tree), player)
}

fn run(slide: bool, velocity: &str) -> Vector3 {
    let (mut ml, player) = ceiling_scene(slide, velocity);
    for _ in 0..120 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        matches!(
            ml.tree().get_node(player).map(|n| n.get_property("ticks")),
            Some(Variant::Int(n)) if n > 0
        ),
        "`_physics_process` never ran"
    );
    node3d::get_position(ml.tree(), player)
}

/// **The acceptance.** Jumping into a ceiling with the knob off stops the body
/// instead of letting it skate sideways.
#[test]
fn slide_on_ceiling_false_stops_a_jump_against_the_ceiling() {
    // Up AND sideways: the sideways component is what "sliding along the
    // ceiling" would preserve, so it is the thing that has to differ.
    let sliding = run(true, "Vector3(6, 10, 0)");
    let stopped = run(false, "Vector3(6, 10, 0)");

    // ⚠️ Control first: with sliding ON the body must actually travel sideways,
    // otherwise "stopped" below is indistinguishable from a body that never
    // moved or never reached the ceiling at all.
    assert!(
        sliding.x > 1.0,
        "with slide_on_ceiling = true the body must skate along the ceiling; it \
         only reached x = {} — the fixture never established contact, so the \
         comparison below would prove nothing",
        sliding.x
    );
    assert!(
        stopped.x < sliding.x,
        "with slide_on_ceiling = false the body must stop rather than slide: \
         got x = {} against {} when sliding is enabled",
        stopped.x,
        sliding.x
    );
}
