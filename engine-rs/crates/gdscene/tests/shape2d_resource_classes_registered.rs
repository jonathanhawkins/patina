//! pat-s3rrv.188 — the `Shape2D` resource subtree is in ClassDB, and a
//! script-built shape produces the same collider as a `.tscn` sub-resource.
//!
//! ⓘ **This bead's implementation already existed; what was missing was this
//! test.** All nine classes are registered with the right parents and doc-XML
//! defaults, and the Resource `.new()` path was built by pat-s3rrv.98 (whose own
//! comment cites `CircleShape2D.new(); s.radius = 40` as its motivating case).
//! The bead's claims to the contrary — "NOT ONE of the nine classes is in
//! ClassDB", "`WorldBoundaryShape2D.new()` errors", "`is_node_class_name`
//! (interpreter.rs:21256) is a hardcoded node-name list" — were all stale. The
//! real gap was that nothing pinned any of it, so the whole subtree was one
//! refactor away from regressing silently.
//!
//! 🔑 The two construction paths converge on ONE match in
//! `physics_server.rs`: a `.tscn` sub-resource is flattened to
//! `__shape__class` + `__shape__<key>` (`packed_scene.rs:1448`), while a
//! script-built shape arrives as `Variant::Resource(r)` carrying `r.class_name`
//! and its own property bag. That shared arm is what makes the equivalence below
//! meaningful rather than a coincidence.

use gdcore::math::Vector2;
use gdobject::class_db::{class_exists, get_property_list, is_abstract, is_parent_class};
use gdphysics2d::shape::Shape2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::physics_server::PhysicsServer;
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdvariant::Variant;

/// The eight concrete leaves, with the doc-XML defaults the acceptance names.
const LEAVES: &[&str] = &[
    "CircleShape2D",
    "RectangleShape2D",
    "CapsuleShape2D",
    "SegmentShape2D",
    "SeparationRayShape2D",
    "ConvexPolygonShape2D",
    "ConcavePolygonShape2D",
    "WorldBoundaryShape2D",
];

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

/// A static body whose collision shape is a `.tscn` WorldBoundaryShape2D.
const TSCN: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="WorldBoundaryShape2D" id="WB_1"]
normal = Vector2(0, -1)
distance = 25.0

[node name="World" type="Node2D"]

[node name="FromScene" type="StaticBody2D" parent="."]

[node name="Col" type="CollisionShape2D" parent="FromScene"]
shape = SubResource("WB_1")

[node name="FromScript" type="StaticBody2D" parent="."]

[node name="Col" type="CollisionShape2D" parent="FromScript"]
"#;

/// Builds the same shape at runtime and assigns it to the second body.
///
/// ⚠️ Deliberately the spelling a Godot tutorial uses — `.new()`, then property
/// writes, then assignment to the node. Constructing the `Variant::Resource` in
/// Rust would prove the physics arm reads a resource, which was never in doubt;
/// only the script path exercises ClassDB construction and the property bag it
/// is seeded with.
const BUILDER_GD: &str = "\
extends Node2D
func run():
    var s = WorldBoundaryShape2D.new()
    s.normal = Vector2(0, -1)
    s.distance = 25.0
    var col = get_node(\"FromScript/Col\")
    col.shape = s
";

/// Every registered body's shape.
///
/// ⚠️ Collected rather than indexed. `bodies()` iterates the world's own
/// storage, whose order is not promised to match scene order — a positional
/// `nth(0)`/`nth(1)` would be a coin flip that passes locally and fails
/// somewhere else. The claim under test is "the two paths agree", which is
/// order-independent by nature, so the assertion should be too.
fn all_shapes(server: &PhysicsServer) -> Vec<Shape2D> {
    server.world().bodies().map(|b| b.shape).collect()
}

/// **The acceptance test.** ClassDB introspection, then the two paths agreeing.
#[test]
fn shape2d_resource_classes_registered() {
    gdobject::class_db::register_all_classes();

    // -- ClassDB reports the subtree -----------------------------------------
    assert!(class_exists("Shape2D"), "Shape2D must be registered");
    assert!(
        is_abstract("Shape2D"),
        "Shape2D is abstract in Godot — a script must not be able to construct it"
    );
    assert!(is_parent_class("Shape2D", "Resource"));

    for leaf in LEAVES {
        assert!(class_exists(leaf), "{leaf} must be registered");
        assert!(
            is_parent_class(leaf, "Shape2D"),
            "{leaf} must derive Shape2D, not Resource directly"
        );
        // 🔑 The `.new()` gate is `is_parent_class(name, "RefCounted")`
        // (`interpreter.rs:3805`), reached through Shape2D -> Resource ->
        // RefCounted. If any link in that chain were missing the class would
        // register fine and still be unconstructible from a script.
        assert!(
            is_parent_class(leaf, "RefCounted"),
            "{leaf} must reach RefCounted, or `{leaf}.new()` cannot construct it"
        );
        assert!(!is_abstract(leaf), "{leaf} is concrete");
    }

    // The defaults the acceptance names explicitly.
    assert_eq!(
        default_of("WorldBoundaryShape2D", "normal"),
        Some(Variant::Vector2(Vector2::new(0.0, -1.0))),
        "Godot's default normal is (0, -1)"
    );
    assert_eq!(
        default_of("WorldBoundaryShape2D", "distance"),
        Some(Variant::Float(0.0))
    );
    // Two more leaves, so "the defaults are right" is not a claim about one class.
    assert_eq!(
        default_of("CircleShape2D", "radius"),
        Some(Variant::Float(10.0))
    );
    assert_eq!(
        default_of("RectangleShape2D", "size"),
        Some(Variant::Vector2(Vector2::new(20.0, 20.0)))
    );

    // -- the two construction paths agree ------------------------------------
    let scene = PackedScene::from_tscn(TSCN).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let main = tree
        .get_node_by_path("/root/World")
        .expect("scene root instanced");
    tree.attach_script(
        main,
        Box::new(GDScriptNodeInstance::from_source(BUILDER_GD, main).unwrap()),
    );
    // Driven through `call_script_method`, which installs the scene accessor the
    // `get_node(..)` inside the script needs.
    tree.call_script_method(main, "run", &[]);

    // The script really did assign a resource — otherwise the body below falls
    // back to some default shape and the equality assertion passes vacuously.
    let script_col = tree
        .get_node_by_path("/root/World/FromScript/Col")
        .expect("second collision shape instanced");
    assert!(
        matches!(
            tree.get_node(script_col).map(|n| n.get_property("shape")),
            Some(Variant::Resource(_))
        ),
        "`WorldBoundaryShape2D.new()` must yield a Resource the script can assign"
    );

    let mut server = PhysicsServer::new();
    server.register_bodies(&tree);
    assert_eq!(server.body_count(), 2, "both static bodies must register");

    let shapes = all_shapes(&server);
    assert_eq!(shapes.len(), 2);
    assert_eq!(
        shapes[0], shapes[1],
        "the .tscn sub-resource and the script-built shape must produce the SAME collider"
    );
    // And it is the RIGHT shape, not two matching wrong ones — without this,
    // both paths failing identically (e.g. both falling back to a default)
    // would read as agreement.
    let expected = Shape2D::world_boundary(Vector2::new(0.0, -1.0), 25.0);
    assert_eq!(
        shapes[0], expected,
        "both paths must yield the authored half-plane"
    );
}

/// `Shape2D` itself cannot be constructed.
///
/// ⚠️ The abstract check runs BEFORE the RefCounted branch in
/// `interpreter.rs`, and it is what stops `Shape2D.new()` from succeeding
/// through the generic Resource path. A base registered without
/// `.abstract_class()` would become constructible — a ClassDB data gap that
/// only an assertion like this catches.
#[test]
fn the_abstract_base_is_not_constructible() {
    gdobject::class_db::register_all_classes();
    assert!(is_abstract("Shape2D"));
    assert!(
        !is_abstract("WorldBoundaryShape2D"),
        "the leaves must stay concrete"
    );
}
