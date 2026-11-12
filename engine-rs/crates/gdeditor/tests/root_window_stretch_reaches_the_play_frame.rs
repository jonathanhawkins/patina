//! pat-nzgdu.87 acceptance: the root window's content scale actually reaches
//! the running (Play) frame, and reaches CanvasLayer items too.
//!
//! `gdscene/tests/root_window_content_scale_pipeline` proves the settings reach
//! the transform. This proves the transform is *applied* — the "wired but
//! inert" failure the port audit keeps finding, where a computed value has no
//! consumer.
//!
//! ⚠️ The distinction under test: `apply_camera2d_view` deliberately SKIPS
//! items on a `CanvasLayer` (a HUD must not scroll with the world), but the
//! stretch transform must apply to them. It maps the whole finished viewport
//! onto the window, so a HUD that ignored it would sit at 1:1 while the game
//! scaled around it.

use gdcore::math::{Transform2D, Vector2};
use gdeditor::viewport_2d_render::{
    apply_stretch_transform, build_runtime_render_list, Render2DItem,
};
use gdscene::node::Node;
use gdscene::node2d;
use gdscene::scene_tree::SceneTree;
use gdscene::viewport::RootWindow;
use gdvariant::Variant;

use gdcore::content_scale::{ContentScaleAspect, ContentScaleConfig, ContentScaleMode};

/// A scene with a world sprite and a HUD sprite parented under a CanvasLayer.
fn scene() -> (SceneTree, u64, u64) {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let world = tree
        .add_child(root, Node::new("World", "Sprite2D"))
        .unwrap();
    node2d::set_position(&mut tree, world, Vector2::new(100.0, 50.0));

    let layer = tree
        .add_child(root, Node::new("HUD", "CanvasLayer"))
        .unwrap();
    tree.get_node_mut(layer)
        .unwrap()
        .set_property("layer", Variant::Int(1));
    let hud = tree
        .add_child(layer, Node::new("Score", "Sprite2D"))
        .unwrap();
    node2d::set_position(&mut tree, hud, Vector2::new(10.0, 10.0));

    (tree, world.raw(), hud.raw())
}

fn origin_of(items: &[Render2DItem], node_id: u64) -> Option<Vector2> {
    items
        .iter()
        .find(|i| i.node_id == node_id)
        .map(|i| i.global_transform.origin)
}

fn canvas_layer_of(items: &[Render2DItem], node_id: u64) -> i64 {
    items
        .iter()
        .find(|i| i.node_id == node_id)
        .map(|i| i.canvas_layer)
        .expect("item is in the render list")
}

fn stretching_window() -> RootWindow {
    RootWindow {
        size: Vector2::new(1280.0, 720.0),
        config: ContentScaleConfig {
            mode: ContentScaleMode::CanvasItems,
            aspect: ContentScaleAspect::Keep,
            base_size: Vector2::new(640.0, 360.0),
            ..ContentScaleConfig::default()
        },
    }
}

/// **The acceptance test.** With stretching on, every drawn item moves; with it
/// off, nothing moves.
#[test]
fn root_window_stretch_reaches_the_play_frame() {
    let (tree, world_id, hud_id) = scene();

    let unstretched = build_runtime_render_list(&tree);
    let world_before = origin_of(&unstretched, world_id).expect("world sprite is drawn");
    let hud_before = origin_of(&unstretched, hud_id).expect("HUD sprite is drawn");

    // ⚠️ Guard against a vacuous pass: the HUD assertion below is only
    // meaningful if this item really is on a CanvasLayer. If the layer stopped
    // being detected, the "stretch applies to canvas layers too" claim would
    // pass while testing nothing.
    assert_eq!(
        canvas_layer_of(&unstretched, world_id),
        0,
        "the world sprite is on the default canvas"
    );
    assert_eq!(
        canvas_layer_of(&unstretched, hud_id),
        1,
        "the HUD sprite must be detected as sitting on CanvasLayer(layer=1)"
    );

    // A window with no stretching must leave the frame byte-identical.
    let mut untouched = build_runtime_render_list(&tree);
    apply_stretch_transform(&mut untouched, RootWindow::default().stretch_transform());
    assert_eq!(
        origin_of(&untouched, world_id),
        Some(world_before),
        "stretch/mode=disabled must not move anything"
    );

    // 640x360 base in a 1280x720 window is a clean 2x.
    let mut stretched = build_runtime_render_list(&tree);
    apply_stretch_transform(&mut stretched, stretching_window().stretch_transform());

    let world_after = origin_of(&stretched, world_id).expect("world sprite is drawn");
    assert!(
        (world_after.x - world_before.x * 2.0).abs() < 0.01
            && (world_after.y - world_before.y * 2.0).abs() < 0.01,
        "the world sprite must be scaled 2x by the stretch: {world_before:?} -> {world_after:?}"
    );

    // ⚠️ The CanvasLayer item must scale too — unlike the camera view.
    let hud_after = origin_of(&stretched, hud_id).expect("HUD sprite is drawn");
    assert!(
        (hud_after.x - hud_before.x * 2.0).abs() < 0.01
            && (hud_after.y - hud_before.y * 2.0).abs() < 0.01,
        "a HUD on a CanvasLayer must be stretched with everything else, or it \
         sits at 1:1 while the game scales around it: {hud_before:?} -> {hud_after:?}"
    );
}

/// An identity transform is a no-op fast path, not a silent rewrite.
#[test]
fn identity_stretch_leaves_every_item_untouched() {
    let (tree, world_id, hud_id) = scene();
    let before = build_runtime_render_list(&tree);
    let mut after = build_runtime_render_list(&tree);
    apply_stretch_transform(&mut after, Transform2D::IDENTITY);

    for id in [world_id, hud_id] {
        assert_eq!(origin_of(&before, id), origin_of(&after, id));
    }
}

/// A letterboxed window offsets every item by the bar thickness, so the game
/// image sits inside the bars rather than under them.
#[test]
fn letterbox_margin_offsets_the_drawn_items() {
    let (tree, world_id, _) = scene();
    let window = RootWindow {
        // 4:3 window around the 16:9 base: 60px bars top and bottom.
        size: Vector2::new(640.0, 480.0),
        config: ContentScaleConfig {
            mode: ContentScaleMode::CanvasItems,
            aspect: ContentScaleAspect::Keep,
            base_size: Vector2::new(640.0, 360.0),
            ..ContentScaleConfig::default()
        },
    };
    assert_eq!(window.content_scale().margin, Vector2::new(0.0, 60.0));

    let mut items = build_runtime_render_list(&tree);
    apply_stretch_transform(&mut items, window.stretch_transform());

    // The scale here is 1x (640 wide either way), so the only change is the bar
    // offset — which makes this a clean read of the margin alone.
    let after = origin_of(&items, world_id).expect("world sprite is drawn");
    assert!(
        (after.y - (50.0 + 60.0)).abs() < 0.01,
        "the item must be pushed down by the top bar: {after:?}"
    );
}
