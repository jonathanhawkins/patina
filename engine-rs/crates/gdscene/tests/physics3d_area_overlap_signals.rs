//! pat-0q2kx.35 acceptance: `Area3D` nodes take part in 3D physics.
//!
//! The library had a complete `Area3D` (`gdphysics3d/area3d.rs`) and the world
//! already folded area gravity overrides into its step, but **no Area3D node was
//! ever registered** — `gdscene` did not mention the class, so overlap detection
//! and space overrides were unreachable from a scene. These tests drive real
//! `MainLoop::step` frames and assert on tree/script state.
//!
//! Note the 3D server registers lazily inside `sync_to_physics`. The 2D server
//! does too since pat-s3rrv.25; before that it needed an explicit
//! `register_physics_bodies()` and never unregistered anything.

use gdcore::math::Vector3;
use gdobject::signal::Connection;
use gdscene::main_loop::MainLoop;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{node3d, GDScriptNodeInstance};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Counts the two overlap signals. Attached to the scene root, which is the
/// connection target — the same shape a `.tscn` `[connection … to="."]` produces,
/// and it dispatches through the same script path (`scene_tree.rs`: connections
/// without callbacks go to the target node's script).
const ROOT_GD: &str = "\
extends Node3D
var entered = 0
var exited = 0
func _on_body_entered(body):
    self.entered = self.entered + 1
func _on_body_exited(body):
    self.exited = self.exited + 1
";

/// Reads the script-facing overlap query from inside the running scene.
const ZONE_GD: &str = "\
extends Area3D
var saw_overlap = false
func _physics_process(delta):
    if has_overlapping_bodies():
        self.saw_overlap = true
";

fn script_var(ml: &MainLoop, node: NodeId, name: &str) -> Option<Variant> {
    ml.tree()
        .get_script(node)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.default_value)
}

/// Adds a `CollisionShape3D` child carrying a shape class name plus its
/// dimensions — what `shape_from_node_3d` reads.
fn add_shape(tree: &mut SceneTree, parent: NodeId, class: &str, dim: (&str, Variant)) {
    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String(class.to_string()));
    shape.set_property(dim.0, dim.1);
    tree.add_child(parent, shape).unwrap();
}

/// `Root -> {Zone (Area3D, 4×4×4 box at the origin), Ball (RigidBody3D sphere)}`
/// with the ball starting at `ball_y`. Returns `(loop, root, zone, ball)`.
fn area_scene(
    ball_y: f32,
    zone_props: &[(&str, Variant)],
    scripts: bool,
) -> (MainLoop, NodeId, NodeId, NodeId) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let zone = tree.add_child(root, Node::new("Zone", "Area3D")).unwrap();
    node3d::set_position(&mut tree, zone, Vector3::ZERO);
    add_shape(
        &mut tree,
        zone,
        "BoxShape3D",
        ("size", Variant::Vector3(Vector3::new(4.0, 4.0, 4.0))),
    );
    for (key, value) in zone_props {
        if let Some(node) = tree.get_node_mut(zone) {
            node.set_property(key, value.clone());
        }
    }

    let ball = tree
        .add_child(root, Node::new("Ball", "RigidBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, ball, Vector3::new(0.0, ball_y, 0.0));
    add_shape(
        &mut tree,
        ball,
        "SphereShape3D",
        ("radius", Variant::Float(0.5)),
    );

    if scripts {
        // A method-name connection (no callback) dispatches to the target node's
        // script, which is how a `.tscn` `[connection]` reaches `_on_body_entered`.
        tree.connect_signal(
            zone,
            "body_entered",
            Connection::new(root.object_id(), "_on_body_entered"),
        );
        tree.connect_signal(
            zone,
            "body_exited",
            Connection::new(root.object_id(), "_on_body_exited"),
        );
        tree.attach_script(
            root,
            Box::new(GDScriptNodeInstance::from_source(ROOT_GD, root).unwrap()),
        );
        tree.attach_script(
            zone,
            Box::new(GDScriptNodeInstance::from_source(ZONE_GD, zone).unwrap()),
        );
    }

    (MainLoop::new(tree), root, zone, ball)
}

/// **The acceptance test.** A rigid sphere falls through an Area3D: the connected
/// script sees exactly one `body_entered` then exactly one `body_exited`, and
/// while the ball is inside the area reports it as overlapping.
#[test]
fn physics3d_area_overlap_signals() {
    let (mut ml, root, zone, ball) = area_scene(6.0, &[], true);

    // Nothing has happened before the first frame.
    assert_eq!(script_var(&ml, root, "entered"), Some(Variant::Int(0)));

    // Fall until the script reports the entry, sampling the overlap set at that
    // moment — `get_overlapping_bodies` answers for the *current* world state, so
    // it is only meaningful while the ball is still inside.
    let mut overlapping_while_inside: Vec<NodeId> = Vec::new();
    for _ in 0..240 {
        ml.step(1.0 / 60.0);
        if script_var(&ml, root, "entered") == Some(Variant::Int(1))
            && script_var(&ml, root, "exited") == Some(Variant::Int(0))
        {
            let seen = ml.physics_server_3d().overlapping_body_nodes(zone);
            if !seen.is_empty() {
                overlapping_while_inside = seen;
            }
        }
    }

    assert_eq!(
        script_var(&ml, root, "entered"),
        Some(Variant::Int(1)),
        "the connected script must see exactly one body_entered",
    );
    assert_eq!(
        script_var(&ml, root, "exited"),
        Some(Variant::Int(1)),
        "…and exactly one body_exited once the ball leaves",
    );
    assert_eq!(
        overlapping_while_inside,
        vec![ball],
        "get_overlapping_bodies() must report the ball while it is inside the area",
    );
    assert_eq!(
        script_var(&ml, zone, "saw_overlap"),
        Some(Variant::Bool(true)),
        "has_overlapping_bodies() must be reachable from GDScript and answer true \
         while the ball is inside",
    );

    // The ball ended below the zone, so the exit was a real departure rather than
    // the body being deleted or stalled inside.
    let end = node3d::get_position(ml.tree(), ball);
    assert!(
        end.y < -2.0,
        "the ball should have fallen out the bottom of the 4-unit zone, got {end:?}",
    );
}

/// An `Area3D` whose gravity space override is REPLACE with gravity pointing +Y
/// lifts a body that is inside it, through `MainLoop` ticks.
///
/// The control case matters twice over: it proves the rise is the override's doing,
/// and it pins the enum mapping. Godot's `SPACE_OVERRIDE_REPLACE` is **3**, while
/// the library enum's own `Replace` discriminant is 2 — a cast instead of a mapping
/// would silently make this COMBINE_REPLACE and the ball would keep falling.
#[test]
fn area3d_gravity_space_override_replace_lifts_a_body() {
    // Ball starts inside the zone (origin), so the override applies immediately.
    let lifted = {
        let (mut ml, _root, _zone, ball) = area_scene(
            0.0,
            &[
                ("gravity_space_override", Variant::Int(3)),
                (
                    "gravity_direction",
                    Variant::Vector3(Vector3::new(0.0, 1.0, 0.0)),
                ),
                ("gravity", Variant::Float(20.0)),
            ],
            false,
        );
        for _ in 0..30 {
            ml.step(1.0 / 60.0);
        }
        node3d::get_position(ml.tree(), ball)
    };
    assert!(
        lifted.y > 0.05,
        "REPLACE gravity pointing +Y must lift the body, got {lifted:?}",
    );

    // Same scene, override disabled: world gravity wins and the ball falls.
    let fell = {
        let (mut ml, _root, _zone, ball) = area_scene(0.0, &[], false);
        for _ in 0..30 {
            ml.step(1.0 / 60.0);
        }
        node3d::get_position(ml.tree(), ball)
    };
    assert!(
        fell.y < -0.05,
        "without an override the body must fall under world gravity, got {fell:?}",
    );
}

/// An Area3D whose node leaves the tree stops being simulated, and one that is not
/// monitoring reports no overlaps — the two states that would otherwise leak.
#[test]
fn area3d_registration_tracks_the_tree_and_honors_monitoring() {
    let (mut ml, _root, zone, _ball) = area_scene(6.0, &[], false);
    ml.step(1.0 / 60.0);
    assert_eq!(
        ml.physics_server_3d().area_count(),
        1,
        "the Area3D node must be registered by the 3D sync",
    );

    // `monitoring = false` silences overlap reporting (Godot's contract) even
    // though the area stays registered.
    if let Some(node) = ml.tree_mut().get_node_mut(zone) {
        node.set_property("monitoring", Variant::Bool(false));
    }
    for _ in 0..240 {
        ml.step(1.0 / 60.0);
    }
    assert!(
        ml.physics_server_3d()
            .overlapping_body_nodes(zone)
            .is_empty(),
        "a non-monitoring Area3D reports no overlapping bodies",
    );

    // Remove the node: the area must be freed rather than left simulating.
    ml.tree_mut().remove_node(zone).unwrap();
    ml.step(1.0 / 60.0);
    assert_eq!(
        ml.physics_server_3d().area_count(),
        0,
        "removing the Area3D node must free its world area",
    );
}
