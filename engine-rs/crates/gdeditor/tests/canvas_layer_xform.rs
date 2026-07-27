//! pat-0q2kx.212 acceptance: `CanvasLayer` transform composition and
//! `follow_viewport`.
//!
//! NOTE ON THE FILE NAME: the acceptance names
//! `canvas_layer_transform_follow_viewport`; that is the test **function**. The
//! binary is `canvas_layer_xform` so the command stays under the verifier's
//! 58-character truncation.
//!
//! Godot ground truth: `scene/main/canvas_layer.cpp` — `_update_xform` composes
//! the layer transform from `offset`/`rotation`/`scale` (or an explicit
//! `transform`), and `_update_follow_viewport` re-parents the layer's canvas to
//! the viewport's with `follow_viewport_scale`.
//!
//! ⚠️ Two things the bead description got wrong, both worth correcting in the
//! tracker:
//!
//! 1. It says `CanvasLayer` is missing from `class_db.rs`. It has been
//!    registered since the ParallaxBackground work (`class_db.rs:6514`) with 6
//!    of its 9 properties; only `transform`, `follow_viewport_scale` and
//!    `custom_viewport` were absent.
//! 2. It calls `gdscene/src/canvas_layer.rs` "unwired". It was worse than that:
//!    the file was **never declared as a module**, so the compiler had never
//!    seen it. Its 14 helpers and its entire test block were dead text.

use gdcore::math::{Transform2D, Vector2};
use gdeditor::viewport_2d_render::{
    apply_camera2d_view, collect_canvas_items, CanvasFollow, Render2DItem,
};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// Parses `tscn` into a tree, with ClassDB populated first.
///
/// ⚠️ The registration is not optional and is not bookkeeping (pat-0q2kx.212).
/// `collect_canvas_items` gates every node on `node.is_class("CanvasItem")`
/// (viewport_2d_render.rs:347), and `Node::is_class` answers a non-exact match
/// purely from `gdobject::is_parent_class` (node.rs:319). With an empty ClassDB
/// there is no ColorRect -> CanvasItem chain, so EVERY node is filtered out and
/// the render list comes back empty — which surfaces as `item_named` panicking
/// with "Hud is not in the render list", not as anything mentioning classes.
///
/// Every test in this file failed that way, and it went unnoticed because both
/// recorded verifies of this bead died compiling a foreign crate, so the tests
/// had never actually executed.
///
/// Registering here rather than per-test also means these tests run the same
/// configuration the editor does (`editor_server.rs:836`), which is what makes
/// the seeded-class-default interaction in `canvas_layer::layer_transform`
/// reachable from a test at all. nextest gives each test its own process, so the
/// global registry cannot leak between them.
fn load(tscn: &str) -> SceneTree {
    gdobject::class_db::register_2d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// `Hud` sits under a transformed CanvasLayer; `World` is a sibling on the
/// default canvas, so it is the control that proves the layer transform is what
/// moved the pixels.
const LAYERED: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Layer" type="CanvasLayer" parent="."]
offset = Vector2(50, 20)
rotation = 0.5

[node name="Hud" type="ColorRect" parent="Layer"]
position = Vector2(10, 0)
size = Vector2(8, 8)

[node name="World" type="ColorRect" parent="."]
position = Vector2(10, 0)
size = Vector2(8, 8)
"#;

fn item_named(items: &[Render2DItem], tree: &SceneTree, name: &str) -> Render2DItem {
    let id = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| tree.get_node(*id).is_some_and(|n| n.name() == name))
        .unwrap_or_else(|| panic!("no node named {name}"));
    items
        .iter()
        .find(|i| i.node_id == id.raw())
        .unwrap_or_else(|| panic!("{name} is not in the render list"))
        .clone()
}

/// **The acceptance.** The layer's offset+rotation move its child; the sibling
/// on the default canvas is untouched.
#[test]
fn canvas_layer_transform_follow_viewport() {
    let tree = load(LAYERED);
    let items = collect_canvas_items(&tree);

    let hud = item_named(&items, &tree, "Hud");
    let world = item_named(&items, &tree, "World");

    // The control first: an identical node outside the layer must be exactly
    // where its own position puts it.
    assert!(
        (world.global_transform.origin - Vector2::new(10.0, 0.0)).length() < 1e-3,
        "the default-canvas sibling must be unaffected, got {:?}",
        world.global_transform.origin
    );

    // The layer transform is `offset` as origin with a rotation basis, so the
    // child's (10, 0) is rotated by 0.5 rad and then translated by (50, 20).
    let expected = Vector2::new(50.0 + 10.0 * 0.5_f32.cos(), 20.0 + 10.0 * 0.5_f32.sin());
    assert!(
        (hud.global_transform.origin - expected).length() < 1e-3,
        "the CanvasLayer offset+rotation must compose into its child: expected \
         {expected:?}, got {:?}. `node2d::get_global_transform` stops at a \
         CanvasLayer (it is not a CanvasItem), so without the explicit \
         composition the layer reads as identity",
        hud.global_transform.origin
    );

    // …and the rotation reached the basis, not just the origin.
    assert!(
        (hud.global_transform.get_rotation() - 0.5).abs() < 1e-3,
        "the layer's rotation must reach the child's basis, got {}",
        hud.global_transform.get_rotation()
    );
}

/// The follow clause. A non-following layer is screen-fixed under a moving
/// camera; a following one shifts with it.
#[test]
fn follow_viewport_decides_whether_the_camera_moves_a_layer() {
    let tree = load(LAYERED);
    let mut items = collect_canvas_items(&tree);
    let before = item_named(&items, &tree, "Hud").global_transform.origin;

    // Default is `follow_viewport_enabled = false`, so the layer is fixed.
    assert_eq!(
        item_named(&items, &tree, "Hud").canvas_follow,
        CanvasFollow::Fixed,
        "a CanvasLayer defaults to screen-fixed"
    );

    let view = Transform2D::translated(Vector2::new(-100.0, -40.0));
    apply_camera2d_view(&mut items, view);

    assert!(
        (item_named(&items, &tree, "Hud").global_transform.origin - before).length() < 1e-3,
        "a non-following layer must stay put under a moving camera — this is \
         what keeps a HUD on screen"
    );
    let world = item_named(&items, &tree, "World");
    assert!(
        (world.global_transform.origin - Vector2::new(10.0, 0.0)).length() > 1.0,
        "the default canvas MUST move with the camera, or the assertion above \
         passes for the wrong reason"
    );
}

/// `follow_viewport_enabled = true` makes the layer track the camera, scaled by
/// `follow_viewport_scale` — Godot's parallax mechanism.
#[test]
fn a_following_layer_tracks_the_camera_scaled() {
    const FOLLOWING: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Layer" type="CanvasLayer" parent="."]
follow_viewport_enabled = true
follow_viewport_scale = 0.5

[node name="Hud" type="ColorRect" parent="Layer"]
size = Vector2(8, 8)
"#;
    let tree = load(FOLLOWING);
    let mut items = collect_canvas_items(&tree);
    assert_eq!(
        item_named(&items, &tree, "Hud").canvas_follow,
        CanvasFollow::Follows(0.5),
        "follow_viewport_enabled + scale must reach the render item"
    );

    apply_camera2d_view(
        &mut items,
        Transform2D::translated(Vector2::new(-100.0, 0.0)),
    );

    // Half the camera's motion, because follow_viewport_scale is 0.5. A layer
    // that followed at full rate — or not at all — fails here, so this pins the
    // scale rather than merely "it moved".
    let x = item_named(&items, &tree, "Hud").global_transform.origin.x;
    assert!(
        (x + 50.0).abs() < 1e-3,
        "a 0.5-scaled following layer must move half as far as the camera: \
         expected -50, got {x}"
    );
}

/// Pins the two ClassDB facts the acceptance silently depends on
/// (pat-0q2kx.212).
///
/// Now that `load` registers, the acceptance above runs in the editor's
/// configuration — but it would still pass if either of these regressed, and
/// then stop testing what it claims to. Both are one assertion each:
///
/// 1. The ColorRect -> CanvasItem chain must resolve, or `collect_canvas_items`
///    filters every node out and the acceptance dies on "not in the render
///    list" with nothing pointing at ClassDB.
/// 2. `transform` must be a *registered* CanvasLayer property. That is what
///    seeds an identity `transform` onto every layer via
///    `packed_scene::node_with_class_defaults`, and therefore what makes
///    `layer_transform`'s skip-identity rule load-bearing rather than
///    theoretical. If registration ever goes away, the acceptance keeps passing
///    while the interaction it guards stops existing — this says so instead.
#[test]
fn classdb_preconditions_the_acceptance_depends_on() {
    gdobject::class_db::register_2d_classes();

    assert!(
        gdobject::is_parent_class("ColorRect", "CanvasItem"),
        "collect_canvas_items gates on is_class(\"CanvasItem\"), which answers \
         from ClassDB — without this chain the render list is empty and every \
         test here fails as \"not in the render list\""
    );
    assert!(
        gdobject::class_db::get_property_list("CanvasLayer")
            .iter()
            .any(|p| p.name == "transform"),
        "`transform` must stay registered: seeding it onto every CanvasLayer is \
         what makes layer_transform's skip-identity rule matter"
    );
}

/// The other half of the rule: a `.tscn` that really does author a non-identity
/// `transform` still wins over the decomposed trio, as Godot's `set_transform`
/// does. Skipping identity must not turn into ignoring the property.
#[test]
fn an_authored_transform_still_overrides_offset_and_rotation() {
    const EXPLICIT: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Layer" type="CanvasLayer" parent="."]
offset = Vector2(50, 20)
rotation = 0.5
transform = Transform2D(1, 0, 0, 1, 7, 3)

[node name="Hud" type="ColorRect" parent="Layer"]
position = Vector2(10, 0)
size = Vector2(8, 8)
"#;

    let tree = load(EXPLICIT);
    let items = collect_canvas_items(&tree);
    let hud = item_named(&items, &tree, "Hud");

    // The authored matrix is a pure translation of (7, 3), so the child lands at
    // (17, 3) with no rotation — not at the offset/rotation composition.
    assert!(
        (hud.global_transform.origin - Vector2::new(17.0, 3.0)).length() < 1e-3,
        "an authored non-identity transform must win over offset/rotation, got \
         {:?}",
        hud.global_transform.origin
    );
    assert!(
        hud.global_transform.get_rotation().abs() < 1e-3,
        "the authored transform carries no rotation, so neither should the \
         child, got {}",
        hud.global_transform.get_rotation()
    );
}
