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

fn load(tscn: &str) -> SceneTree {
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
