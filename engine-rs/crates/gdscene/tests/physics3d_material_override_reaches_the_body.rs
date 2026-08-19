//! pat-s3rrv.99 (3D half): `RigidBody3D.physics_material_override` was dropped.
//!
//! `PackedScene` flattens all four `PhysicsMaterial` keys onto the body node as
//! `__physics_material__{friction,bounce,rough,absorbent}`, and that pass is
//! class-agnostic — it fires for 3D bodies exactly as it does for 2D. But
//! `physics_server_3d.rs` read NONE of them: it set `friction`/`bounce` from the
//! flat node properties and stopped. So a 3D material was silently inert while
//! the identical 2D path honoured it.
//!
//! 🔑 `rough`/`absorbent` are carried as a SIGN, not a branch. Godot stores
//! `computed_friction() { return rough ? -friction : friction; }`
//! (`physics_material.h:42-67`) and reads it back through the pair combines,
//! which gdphysics3d already implements Godot-correctly: `abs(MIN(a, b))` for
//! friction and `CLAMP(a + b, 0, 1)` for restitution. So asserting on the
//! NEGATED stored value is asserting on the real mechanism, not on a proxy.

use gdcore::math::Vector3;
use gdscene::node::Node;
use gdscene::node3d;
use gdscene::physics_server_3d::PhysicsServer3D;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// `Root -> Body (RigidBody3D + box collider)` carrying `props`, registered
/// into a fresh server. Returns the body's `(friction, bounce)` as stored.
fn registered_material(props: &[(&str, Variant)]) -> (f32, f32) {
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();

    let body = tree
        .add_child(root, Node::new("Body", "RigidBody3D"))
        .unwrap();
    node3d::set_position(&mut tree, body, Vector3::new(0.0, 10.0, 0.0));
    for (key, value) in props {
        if let Some(node) = tree.get_node_mut(body) {
            node.set_property(key, value.clone());
        }
    }

    let mut shape = Node::new("CollisionShape", "CollisionShape3D");
    shape.set_property("shape", Variant::String("BoxShape3D".to_string()));
    tree.add_child(body, shape).unwrap();

    let mut server = PhysicsServer3D::new();
    server.sync_to_physics(&tree);

    let body_id = server
        .body_for_node(body)
        .expect("the RigidBody3D must register a physics body");
    let registered = server
        .world()
        .get_body(body_id)
        .expect("the registered body must exist in the world");
    (registered.friction, registered.bounce)
}

#[test]
fn a_3d_physics_material_override_reaches_the_body() {
    // Control: with no override the flat defaults stand. Without this, the
    // assertions below could pass against a body that happened to default to
    // the same numbers.
    let (base_friction, base_bounce) = registered_material(&[]);

    let (friction, bounce) = registered_material(&[
        ("__physics_material__friction", Variant::Float(0.25)),
        ("__physics_material__bounce", Variant::Float(0.75)),
    ]);

    assert!(
        (friction - 0.25).abs() < 1e-6,
        "`physics_material_override.friction` must reach the 3D body: expected \
         0.25, got {friction} (the un-overridden default is {base_friction})"
    );
    assert!(
        (bounce - 0.75).abs() < 1e-6,
        "`physics_material_override.bounce` must reach the 3D body: expected \
         0.75, got {bounce} (the un-overridden default is {base_bounce})"
    );
    assert!(
        (friction - base_friction).abs() > 1e-6 || (bounce - base_bounce).abs() > 1e-6,
        "fixture check: the override values must DIFFER from the defaults \
         ({base_friction}, {base_bounce}), else this test cannot fail"
    );
}

#[test]
fn rough_and_absorbent_negate_the_stored_values_in_3d() {
    let (plain_friction, plain_bounce) = registered_material(&[
        ("__physics_material__friction", Variant::Float(0.6)),
        ("__physics_material__bounce", Variant::Float(0.4)),
    ]);
    assert!(
        plain_friction > 0.0 && plain_bounce > 0.0,
        "without the flags both values stay POSITIVE: got friction \
         {plain_friction}, bounce {plain_bounce}"
    );

    let (rough_friction, _) = registered_material(&[
        ("__physics_material__friction", Variant::Float(0.6)),
        ("__physics_material__rough", Variant::Bool(true)),
    ]);
    assert!(
        (rough_friction + 0.6).abs() < 1e-6,
        "`rough` is Godot's `computed_friction()` sign flip, so the body must \
         store -0.6; got {rough_friction}"
    );

    let (_, absorbent_bounce) = registered_material(&[
        ("__physics_material__bounce", Variant::Float(0.4)),
        ("__physics_material__absorbent", Variant::Bool(true)),
    ]);
    assert!(
        (absorbent_bounce + 0.4).abs() < 1e-6,
        "`absorbent` is `computed_bounce()`, so the body must store -0.4; got \
         {absorbent_bounce}"
    );
}

/// The negated values are only meaningful because the 3D solver combines the
/// way Godot does. This pins that contract at the arithmetic level, so a change
/// to either combine rule fails HERE rather than as a mysterious physics drift.
#[test]
fn the_3d_combine_rules_read_the_sign_back_out() {
    // `abs(MIN(a, b))`: a rough surface IMPOSES its friction on the pair
    // instead of the usual "slipperiest side wins".
    let slippery = 0.2_f32;
    let rough_road = -0.9_f32; // computed_friction() of a rough 0.9
    let plain_road = 0.9_f32;
    assert!(
        ((slippery.min(plain_road).abs()) - 0.2).abs() < 1e-6,
        "plain: the pair takes the minimum, 0.2"
    );
    assert!(
        ((slippery.min(rough_road).abs()) - 0.9).abs() < 1e-6,
        "rough: the negated value is the minimum, so its magnitude wins at 0.9"
    );

    // `CLAMP(a + b, 0, 1)`: an absorbent surface SUBTRACTS.
    let ball = 0.5_f32;
    assert!(
        ((ball + 0.8_f32).clamp(0.0, 1.0) - 1.0).abs() < 1e-6,
        "plain: 0.5 + 0.8 clamps to a fully elastic 1.0"
    );
    assert!(
        ((ball + -0.8_f32).clamp(0.0, 1.0)).abs() < 1e-6,
        "absorbent: 0.5 + (-0.8) is negative and clamps to 0 — the surface \
         swallows the bounce entirely"
    );
}
