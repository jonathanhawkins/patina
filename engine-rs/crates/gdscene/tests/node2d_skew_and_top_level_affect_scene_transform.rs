//! pat-s3rrv.184 acceptance: `Node2D.skew` shears the local transform, and
//! `CanvasItem.top_level` detaches a node from its parent's.
//!
//! `skew` was never read. `node2d::get_local_transform` composed
//! `translated * rotated * scaled` and stopped there, so a skewed Node2D
//! round-tripped through the `.tscn` and the inspector and drew square. The
//! property was not even registered in ClassDB — the only `skew` there was
//! StyleBoxFlat's, which is a `Vector2`.
//!
//! ⚠️ **The `top_level` half was already delivered**, by pat-s3rrv.22, which
//! fixed `get_global_transform` itself (rather than adding a parallel
//! `_top_level` variant as the 3D side had) and shipped four tests in
//! `canvas_item_top_level_ignores_parent_transform.rs`. This bead was written
//! against an older snapshot. The clauses are re-asserted here anyway because
//! the acceptance names them and because they are cheap — but the coverage that
//! matters for `top_level` lives in that file, not this one.
//!
//! ⓘ Named for the acceptance. An earlier revision shortened this to
//! `node2d_skew_top_level` on the belief that the verifier caps commands at 58
//! characters; `verifier.rs` shows it truncates only failure output, so the
//! short name only served to leave the bead's stored target dangling.
//!
//! Ported from `upstream/godot/scene/2d/node_2d.cpp::_update_transform`, which
//! builds the matrix as `Transform2D(rotation, scale, skew, position)`.

use gdcore::math::{Transform2D, Vector2};
use gdscene::node::{Node, NodeId};
use gdscene::node2d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A parent at (500, 500) with a detached and an attached sprite under it.
///
/// ⚠️ Authored as `.tscn` text rather than by setting properties: `top_level`
/// and `skew` both have to survive the loader to be worth anything, and
/// injecting them here would skip exactly the boundary that was broken for
/// `skew` (it never reached ClassDB, so nothing declared its default).
const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="Parent" type="Node2D" parent="."]
position = Vector2(500, 500)

[node name="Detached" type="Sprite2D" parent="Parent"]
top_level = true

[node name="Attached" type="Sprite2D" parent="Parent"]
"#;

/// A skewed node and an unskewed sibling, identical in every other respect.
const SKEW_SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node2D"]

[node name="Skewed" type="Node2D" parent="."]
skew = 0.5

[node name="Square" type="Node2D" parent="."]
"#;

fn tree_from(tscn: &str) -> SceneTree {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn node(tree: &SceneTree, path: &str) -> NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("node '{path}' not instanced"))
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

/// **The acceptance test.** Skew shears the basis; `top_level` detaches.
#[test]
fn node2d_skew_and_top_level_affect_scene_transform() {
    // -- skew -------------------------------------------------------------
    let tree = tree_from(SKEW_SCENE);
    let skewed = node(&tree, "/root/Main/Skewed");
    let square = node(&tree, "/root/Main/Square");

    // The `.tscn` value reached the node. Without it every assertion below
    // passes for the wrong reason — an unread property and a zero skew are
    // indistinguishable downstream.
    assert!(
        approx(node2d::get_skew(&tree, skewed), 0.5),
        "the loader must carry `skew` onto the node, got {}",
        node2d::get_skew(&tree, skewed)
    );
    assert!(approx(node2d::get_skew(&tree, square), 0.0));

    let sheared = node2d::get_local_transform(&tree, skewed);
    let plain = node2d::get_local_transform(&tree, square);

    // 🔑 The acceptance's own wording: the y column's x component is non-zero.
    // At rotation 0 and unit scale the basis is x = (1, 0), y = (-sin 0.5,
    // cos 0.5) = (-0.4794, 0.8776) — Godot shears the Y basis relative to X.
    assert!(
        approx(sheared.y.x, -(0.5_f32).sin()),
        "y column must be sheared, got {:?}",
        sheared.y
    );
    assert!(
        approx(plain.y.x, 0.0),
        "an unskewed node keeps a square basis, got {:?}",
        plain.y
    );
    // ⚠️ The X column is UNTOUCHED by skew. Asserting only "the basis changed"
    // would accept an implementation that rotated the whole frame, which is a
    // different transform that happens to make y.x non-zero too.
    assert!(approx(sheared.x.x, 1.0) && approx(sheared.x.y, 0.0));

    // Corner positions differ, which is what "draws sheared" means in practice.
    let corner = Vector2::new(0.0, 100.0);
    let moved = sheared.xform(corner);
    assert!(
        approx(moved.x, -(0.5_f32).sin() * 100.0) && approx(moved.y, (0.5_f32).cos() * 100.0),
        "a corner must move under skew, got {moved:?}"
    );
    assert!(
        (moved - plain.xform(corner)).length() > 1.0,
        "the skewed and unskewed corners must genuinely differ"
    );

    // -- top_level --------------------------------------------------------
    let tree = tree_from(SCENE);
    let detached = node(&tree, "/root/Main/Parent/Detached");
    let attached = node(&tree, "/root/Main/Parent/Attached");

    let d = node2d::get_global_position(&tree, detached);
    let a = node2d::get_global_position(&tree, attached);
    assert!(
        approx(d.x, 0.0) && approx(d.y, 0.0),
        "a `top_level` sprite ignores its parent's (500, 500), got {d:?}"
    );
    assert!(
        approx(a.x, 500.0) && approx(a.y, 500.0),
        "its ordinary sibling still inherits it, got {a:?}"
    );
}

/// A zero skew composes **exactly** as the previous implementation did.
///
/// 🔑 This is the regression guard for the change, and it is the assertion that
/// matters most: `get_local_transform` is the entire 2D transform path — render,
/// physics and input all read it — so the rewrite from
/// `translated * rotated * scaled` to `from_rotation_scale_skew_origin` must be
/// a no-op for every scene that does not use skew. Both forms expand to
/// `x = (cos*sx, sin*sx)`, `y = (-sin*sy, cos*sy)`, `origin = pos`; this pins
/// that they agree on a rotation and a non-uniform scale, where a transposed or
/// mis-ordered composition would diverge.
#[test]
fn a_zero_skew_transform_matches_the_previous_composition() {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("N", "Node2D")).unwrap();
    // ⚠️ Deliberately not a coincidence point: a non-zero rotation AND a
    // non-uniform scale. At rotation 0 or scale (1,1) a transposed basis and a
    // correct one are identical, so the guard would pass on a broken rewrite.
    node2d::set_position(&mut tree, id, Vector2::new(7.0, -3.0));
    node2d::set_rotation(&mut tree, id, 0.3);
    node2d::set_scale(&mut tree, id, Vector2::new(2.0, 5.0));

    let got = node2d::get_local_transform(&tree, id);
    let expected = Transform2D::translated(Vector2::new(7.0, -3.0))
        * Transform2D::rotated(0.3)
        * Transform2D::scaled(Vector2::new(2.0, 5.0));

    assert!(
        approx(got.x.x, expected.x.x) && approx(got.x.y, expected.x.y),
        "x: {got:?}"
    );
    assert!(
        approx(got.y.x, expected.y.x) && approx(got.y.y, expected.y.y),
        "y: {got:?}"
    );
    assert!(
        approx(got.origin.x, expected.origin.x) && approx(got.origin.y, expected.origin.y),
        "origin: {got:?}"
    );
}

/// A skew authored without a decimal point still reads.
///
/// ⚠️ `skew = 1` in a `.tscn` parses to `Variant::Int`, and a Float-only reader
/// answers 0.0 — turning a type mismatch into "no skew", which is invisible
/// because it looks exactly like the default.
#[test]
fn an_integer_skew_literal_is_not_silently_zero() {
    let tree = tree_from("[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\nskew = 1\n");
    let main = node(&tree, "/root/Main");
    assert!(
        approx(node2d::get_skew(&tree, main), 1.0),
        "an Int literal must read as 1.0, got {}",
        node2d::get_skew(&tree, main)
    );
}

/// `skew` is registered on Node2D, so it has a declared default.
///
/// Registration is what makes the property reflectable — `list_properties`, the
/// inspector, and a script reading it before anything has written one.
#[test]
fn skew_is_registered_on_node2d() {
    gdobject::class_db::register_all_classes();
    let props = gdobject::class_db::get_property_list("Node2D");
    let skew = props
        .iter()
        .find(|p| p.name == "skew")
        .expect("Node2D must declare `skew` — only StyleBoxFlat did before this bead");
    assert!(
        matches!(skew.default_value, Variant::Float(f) if f == 0.0),
        "Godot's default is 0.0, got {:?}",
        skew.default_value
    );
}

/// A `top_level` node's global **scale** ignores its ancestors too.
///
/// 🔑 This is the assertion that would have caught the gap. pat-s3rrv.22 taught
/// `get_global_transform`, `get_global_rotation` and `set_global_position` to
/// stop at a `top_level` node and missed `get_global_scale`, which kept
/// multiplying the whole chain. A detached node therefore reported a scale its
/// own transform did not agree with — the transform ignored the parent and the
/// scale did not.
///
/// ⚠️ The parent's scale is 2 and the child's is 3, not 1: with a unit scale
/// anywhere in the chain the product and the local value coincide and the bug
/// is invisible.
#[test]
fn global_scale_stops_at_a_top_level_node() {
    let tree = tree_from(
        "[gd_scene format=3]\n\n[node name=\"Main\" type=\"Node2D\"]\n\n\
         [node name=\"Parent\" type=\"Node2D\" parent=\".\"]\nscale = Vector2(2, 2)\n\n\
         [node name=\"Detached\" type=\"Sprite2D\" parent=\"Parent\"]\n\
         scale = Vector2(3, 3)\ntop_level = true\n\n\
         [node name=\"Attached\" type=\"Sprite2D\" parent=\"Parent\"]\n\
         scale = Vector2(3, 3)\n",
    );
    let detached = node(&tree, "/root/Main/Parent/Detached");
    let attached = node(&tree, "/root/Main/Parent/Attached");

    let d = node2d::get_global_scale(&tree, detached);
    assert!(
        approx(d.x, 3.0) && approx(d.y, 3.0),
        "a detached node keeps its OWN scale, not 2*3 = 6: got {d:?}"
    );
    // The ordinary sibling still composes, which is what makes the assertion
    // above a statement about `top_level` rather than about scale being ignored.
    let a = node2d::get_global_scale(&tree, attached);
    assert!(
        approx(a.x, 6.0) && approx(a.y, 6.0),
        "an attached sibling still multiplies through: got {a:?}"
    );

    // And the scale agrees with the transform the node actually draws with —
    // the disagreement was the bug, not the number itself.
    let basis_x = node2d::get_global_transform(&tree, detached).x;
    assert!(
        approx(basis_x.length(), 3.0),
        "global scale must match the global transform's basis: got {basis_x:?}"
    );
}

/// A GDScript reads `get_global_position()` off a `top_level` sprite.
///
/// 🔑 The acceptance names this spelling, and it did not work: `scripting.rs`
/// had a Node3D method block (`look_at`, `rotate`, `to_global`) and **nothing
/// for 2D**, so the call raised `UndefinedFunction` while
/// `node2d::get_global_position` computed the right answer one layer down.
/// Registering the method in ClassDB would not have helped — registration makes
/// a method reflectable, but a call still has to land on a dispatch arm.
///
/// ⚠️ `got` is seeded to a NON-zero value. The clause asserts the script sees
/// (0, 0), and a script that died on `UndefinedFunction` before ever assigning
/// would leave a default-constructed zero behind — passing the assertion for
/// exactly the reason the test exists to rule out.
const READER_GD: &str = "\
extends Node2D
var got = Vector2(999, 999)
var sibling = Vector2(999, 999)
func run():
    var d = get_node(\"Parent/Detached\")
    var a = get_node(\"Parent/Attached\")
    self.got = d.get_global_position()
    self.sibling = a.get_global_position()
";

#[test]
fn a_script_reads_global_position_through_top_level() {
    let mut tree = tree_from(SCENE);
    let main = node(&tree, "/root/Main");
    tree.attach_script(
        main,
        Box::new(gdscene::GDScriptNodeInstance::from_source(READER_GD, main).unwrap()),
    );
    // ⚠️ Driven through `call_script_method`, which installs the scene accessor.
    // `get_script_mut(..).call_method(..)` hands back a script with no scene
    // access, and the `get_node(..)` calls inside would raise instead.
    tree.call_script_method(main, "run", &[]);

    let read = |key: &str| match tree.get_node(main).unwrap().get_property(key) {
        Variant::Vector2(v) => v,
        other => panic!(
            "`{key}` should be a Vector2, got {other:?} — a script that \
                         died on UndefinedFunction never assigns it"
        ),
    };

    let got = read("got");
    assert!(
        approx(got.x, 0.0) && approx(got.y, 0.0),
        "a `top_level` sprite reports world (0, 0) to a script, got {got:?}"
    );
    // The ordinary sibling proves the arm returns a real per-node value rather
    // than a constant — both would be (0, 0) if it answered Vector2::ZERO.
    let sibling = read("sibling");
    assert!(
        approx(sibling.x, 500.0) && approx(sibling.y, 500.0),
        "its attached sibling reports (500, 500), got {sibling:?}"
    );
}
