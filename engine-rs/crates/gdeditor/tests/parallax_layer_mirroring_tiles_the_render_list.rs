//! pat-s3rrv.86 item (1) — `ParallaxLayer.motion_mirroring` tiles the layer's
//! render items so a mirrored background actually repeats.
//!
//! The wrap arithmetic was already correct: `gdserver2d`'s `mirror_axis` wraps
//! the layer's scroll offset, so a mirrored background scrolled at the right
//! speed — and then ran out, because the layer was emitted ONCE. Nothing in
//! `viewport_2d_render.rs` referenced `motion_mirroring` at all.
//!
//! ⚠️ `register_2d_classes()` is not bookkeeping. `collect_canvas_items` gates
//! every node on `node.is_class("CanvasItem")`, which resolves through ClassDB —
//! with an empty registry the render list comes back EMPTY and every assertion
//! below would be comparing nothing to nothing.

use gdcore::math::Vector2;
use gdeditor::viewport_2d_render::{build_runtime_render_list, Render2DItem};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

fn scene(mirroring: &str) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="BG" type="ParallaxBackground" parent="."]

[node name="Layer" type="ParallaxLayer" parent="BG"]
motion_mirroring = {mirroring}

[node name="Art" type="ColorRect" parent="BG/Layer"]
size = Vector2(64, 64)
"#
    )
}

/// The render list plus the raw id of the `Art` node.
///
/// ⚠️ The id is resolved from the TREE by path, not inferred from the list.
/// Picking "the node with the most items" would work for the tiled case and
/// silently choose an arbitrary node in the un-tiled one, where every node has
/// exactly one item — so the baseline assertions would be about whichever node
/// happened to come first.
fn render_list(mirroring: &str) -> (Vec<Render2DItem>, u64) {
    gdobject::class_db::register_2d_classes();
    let parsed = PackedScene::from_tscn(&scene(mirroring)).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &parsed).expect("instance scene");
    let art = tree
        .get_node_by_path("/root/Root/BG/Layer/Art")
        .expect("the Art node is in the scene");
    (build_runtime_render_list(&tree), art.raw())
}

/// Items belonging to the `Art` node.
fn art_items(items: &[Render2DItem], art: u64) -> Vec<&Render2DItem> {
    items.iter().filter(|i| i.node_id == art).collect()
}

/// **The acceptance test.** A mirrored layer is emitted many times, at exact
/// multiples of the stride.
#[test]
fn parallax_layer_mirroring_tiles_the_render_list() {
    let (base, art_id) = render_list("Vector2(0, 0)");
    // Positive control: without this, two empty lists compare equal and the
    // stride assertions below prove nothing.
    assert!(
        !base.is_empty(),
        "the fixture must produce a render list at all — an empty one means \
         ClassDB was not populated and every node was filtered out",
    );
    let base_art = art_items(&base, art_id);
    assert_eq!(
        base_art.len(),
        1,
        "with mirroring (0, 0) the layer is drawn exactly once; got {}",
        base_art.len(),
    );
    let origin = base_art[0].global_transform.origin;

    let (tiled, tiled_art_id) = render_list("Vector2(100, 0)");
    let tiled_art = art_items(&tiled, tiled_art_id);
    assert!(
        tiled_art.len() > 1,
        "a mirrored layer must be emitted more than once — got {}, which is the \
         'scrolls correctly then runs out' bug",
        tiled_art.len(),
    );

    // 🔑 Every copy sits at an exact multiple of the stride along x, and none
    // moves along y: the stride is (100, 0), so y must not repeat.
    for item in &tiled_art {
        let dx = item.global_transform.origin.x - origin.x;
        let dy = item.global_transform.origin.y - origin.y;
        assert!(
            (dx / 100.0).fract().abs() < 1e-3,
            "copies must land on multiples of the 100px stride; got dx = {dx}",
        );
        assert!(
            dy.abs() < 1e-3,
            "a stride of 0 on y means DO NOT REPEAT that axis, not 'repeat every \
             0 pixels'; got dy = {dy}",
        );
    }

    // The original is still among them, exactly once.
    let at_origin = tiled_art
        .iter()
        .filter(|i| (i.global_transform.origin.x - origin.x).abs() < 1e-3)
        .count();
    assert_eq!(
        at_origin, 1,
        "the un-offset original must still be emitted once, not duplicated",
    );
}

/// 🔑 A zero stride on both axes disables the whole pass.
///
/// ⚠️ This is the guard against dividing by the stride. Godot clamps the
/// property to `>= 0` and treats zero as "no repeat"; computing a copy count
/// from it would divide by zero and hang rather than draw wrongly.
#[test]
fn zero_mirroring_emits_no_copies() {
    let (items, art_id) = render_list("Vector2(0, 0)");
    assert_eq!(
        art_items(&items, art_id).len(),
        1,
        "mirroring (0, 0) must leave the layer drawn once",
    );

    // A negative value is clamped to 0 by Godot (`parallax_layer.cpp:81`), so it
    // must behave identically rather than tiling backwards.
    let (negative, neg_id) = render_list("Vector2(-100, -100)");
    assert_eq!(
        art_items(&negative, neg_id).len(),
        1,
        "a negative stride is clamped to zero, so it must not tile",
    );
}

/// Both axes repeat when both strides are set.
#[test]
fn both_axes_tile_when_both_strides_are_set() {
    let (items, art_id) = render_list("Vector2(100, 80)");
    let art = art_items(&items, art_id);
    let origin = art
        .iter()
        .map(|i| i.global_transform.origin)
        .fold(Vector2::new(f32::MAX, f32::MAX), |a, b| {
            Vector2::new(a.x.min(b.x), a.y.min(b.y))
        });

    assert!(
        art.iter()
            .any(|i| (i.global_transform.origin.y - origin.y).abs() > 1e-3),
        "with a non-zero y stride the layer must also repeat vertically",
    );
    for item in &art {
        let dy = item.global_transform.origin.y - origin.y;
        assert!(
            (dy / 80.0).fract().abs() < 1e-3,
            "vertical copies must land on multiples of the 80px stride, got {dy}",
        );
    }
}
