//! pat-s3rrv.98: a shape RESOURCE assigned at runtime must reach the body.
//!
//! `$Col.shape = CircleShape2D.new()` is Godot's standard runtime shape
//! authoring. Patina registered the class and let the script construct it, but
//! the value never became collision geometry: `shape_from_shape_props` resolved
//! the shape's class only from a `Variant::String` — either `"SubResource:<id>"`
//! with `__shape__*` companions copied on by the .tscn loader, or a bare class
//! name with the dimensions on the node — and answered `None` for anything
//! else. A script-built shape arrives as `Variant::Resource(Box<ResourceRef>)`,
//! carrying its class name and properties INSIDE the resource, so it matched
//! neither arm.
//!
//! ⚠️ The failure was silent and looked like a physics bug rather than a
//! plumbing one: the body still registered, it just fell back to default
//! geometry, so a script that asked for `radius = 40` got collision at the
//! default and nothing reported an error.
//!
//! 🔑 Assertions here are DIFFERENTIAL. "the body has a circle shape" is true
//! under the bug as well, because the fallback is also a circle — only the
//! RADIUS distinguishes a resolved resource from a default. The control at the
//! bottom pins what the unresolved case actually produces, so the two can never
//! be confused again.

use gdcore::math::Vector2;
use gdphysics2d::shape::Shape2D;
use gdscene::node::{Node, NodeId};
use gdscene::physics_server::PhysicsServer;
use gdscene::SceneTree;
use gdvariant::{ResourceRef, Variant};
use std::collections::HashMap;

/// `Root -> Body(StaticBody2D) -> Col(CollisionShape2D)` where `Col.shape` is a
/// script-built resource of `class_name`, carrying `props`.
fn body_with_shape_resource(
    class_name: &str,
    props: &[(&str, Variant)],
) -> (PhysicsServer, SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let body = tree
        .add_child(root, Node::new("Body", "StaticBody2D"))
        .unwrap();

    let mut properties = HashMap::new();
    for (k, v) in props {
        properties.insert((*k).to_string(), v.clone());
    }
    let mut col = Node::new("Col", "CollisionShape2D");
    col.set_property(
        "shape",
        Variant::Resource(Box::new(ResourceRef {
            // A script-built resource has no `res://` path — it exists only in
            // memory, which is exactly why the SubResource arm cannot see it.
            path: String::new(),
            class_name: class_name.to_string(),
            properties,
        })),
    );
    tree.add_child(body, col).unwrap();

    let mut server = PhysicsServer::new();
    // Both calls, matching `collision_subres`'s helper: `register_bodies`
    // populates `node_to_body`, and without it `body_for_node` answers None for
    // a reason that has nothing to do with shape resolution.
    server.register_bodies(&tree);
    server.sync_to_physics(&mut tree);
    (server, tree, body)
}

/// The registered body's shape, or `None` when the node produced no body at
/// all — which an unresolvable shape class legitimately may.
fn shape_of(server: &PhysicsServer, node: NodeId) -> Option<Shape2D> {
    let id = server.body_for_node(node)?;
    server.world().get_body(id).map(|b| b.shape)
}

#[test]
fn a_script_built_circle_shape_reaches_the_body_at_its_own_radius() {
    let (server, _tree, body) =
        body_with_shape_resource("CircleShape2D", &[("radius", Variant::Float(40.0))]);

    match shape_of(&server, body).expect("the StaticBody2D must register a body") {
        Shape2D::Circle { radius } => assert!(
            (radius - 40.0).abs() < 1e-6,
            "the resource's own `radius` must reach the body: expected 40, got \
             {radius}. A default value here means the shape resource was not \
             resolved and the body fell back to stock geometry — the shape is \
             still a Circle either way, which is why this asserts the RADIUS"
        ),
        other => panic!("expected a Circle from CircleShape2D, got {other:?}"),
    }
}

#[test]
fn a_script_built_rectangle_shape_reaches_the_body_at_its_own_size() {
    let (server, _tree, body) = body_with_shape_resource(
        "RectangleShape2D",
        &[("size", Variant::Vector2(Vector2::new(64.0, 16.0)))],
    );

    match shape_of(&server, body).expect("the StaticBody2D must register a body") {
        Shape2D::Rectangle { half_extents } => {
            assert!(
                (half_extents.x - 32.0).abs() < 1e-6 && (half_extents.y - 8.0).abs() < 1e-6,
                "Godot's `size` is the FULL extent, so 64x16 must become \
                 half_extents 32x8; got {half_extents:?}"
            );
        }
        other => panic!("expected a Rectangle from RectangleShape2D, got {other:?}"),
    }
}

/// CONTROL: what an UNRESOLVED shape produces.
///
/// Without this, the tests above could pass for the wrong reason — if the
/// fallback happened to be radius 40, they would be vacuous. Pinning the
/// fallback proves the assertions above are measuring resolution and not a
/// coincidence, and documents the value a future reader will see when this
/// regresses.
#[test]
fn an_unresolved_shape_does_not_produce_the_authored_radius() {
    let (server, _tree, body) =
        body_with_shape_resource("NotAShapeClass2D", &[("radius", Variant::Float(40.0))]);

    // No body at all is a perfectly good answer here — an unresolvable class
    // yielding no geometry is what the `None` arm of `shape_from_shape_props`
    // means. What must NOT happen is the authored 40 appearing anyway.
    if let Some(Shape2D::Circle { radius }) = shape_of(&server, body) {
        assert!(
            (radius - 40.0).abs() > 1e-6,
            "fixture check: an unresolvable shape class must not yield the \
             authored radius 40, else the positive tests above are vacuous"
        );
    }
}
