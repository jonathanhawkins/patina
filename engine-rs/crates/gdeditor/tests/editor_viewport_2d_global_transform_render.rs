//! pat-0q2kx.102: the edit-mode 2D viewport draws nodes where Play draws them.
//!
//! Godot's 2D editor renders each CanvasItem at its composed **global**
//! transform, skips hidden subtrees, and honours draw order
//! (upstream/godot/editor/plugins/canvas_item_editor_plugin.cpp). Patina's edit
//! view positioned every node by its raw local `position` property, ignoring the
//! parent chain, visibility and z — so any scene built out of parent transforms
//! looked wrong in edit mode while Play (`viewport_2d_render.rs`) showed it
//! correctly. Two views of one scene disagreeing is the bug.
//!
//! These assert on the rendered **pixels** of `render_scene_with_zoom_pan`, the
//! function the served editor viewport actually calls, rather than on any
//! intermediate list — so "the transform is computed but not used" fails.

use gdcore::math::Vector2;
use gdeditor::scene_renderer::{compute_scene_bounds, render_scene_with_zoom_pan};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;
use gdvariant::Variant;

const W: u32 = 400;
const H: u32 = 300;

/// Whether two renders differ anywhere within `radius` of `(cx, cy)`.
///
/// Every assertion here is **differential** — one render against another that
/// differs by exactly the node under test — rather than "is this pixel the
/// background colour". The viewport also paints a grid and an origin crosshair,
/// so an absolute background test would call grid lines "content" and pass
/// trivially wherever a gridline happened to fall.
fn differs_near(a: &FrameBuffer, b: &FrameBuffer, cx: f32, cy: f32, radius: i32) -> bool {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let x = cx as i32 + dx;
            let y = cy as i32 + dy;
            if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                continue;
            }
            if a.get_pixel(x as u32, y as u32) != b.get_pixel(x as u32, y as u32) {
                return true;
            }
        }
    }
    false
}

/// Maps a scene-space point to the viewport pixel the renderer would place it
/// at, using the same centring the render pass computes.
fn scene_to_pixel(tree: &SceneTree, p: Vector2) -> (f32, f32) {
    let bounds = compute_scene_bounds(tree);
    let cx = bounds.position.x + bounds.size.x / 2.0;
    let cy = bounds.position.y + bounds.size.y / 2.0;
    let off_x = W as f32 / 2.0 - cx;
    let off_y = H as f32 / 2.0 - cy;
    (p.x + off_x, p.y + off_y)
}

fn node2d(name: &str, pos: Vector2) -> Node {
    let mut n = Node::new(name, "Node2D");
    n.set_property("position", Variant::Vector2(pos));
    n
}

/// `root -> Parent(100,100 rot 90°) -> Child sprite(50,0)`.
///
/// A 90° rotation sends the child's local `(50, 0)` to `(0, 50)`, so its global
/// position is `(100, 150)` — **not** `(150, 100)`, which is what summing local
/// positions without the rotation would give. The two differ, which is what
/// makes this test able to tell them apart.
fn rotated_parent_scene(visible: bool) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let mut parent = node2d("Parent", Vector2::new(100.0, 100.0));
    parent.set_property("rotation", Variant::Float(std::f64::consts::FRAC_PI_2));
    if !visible {
        parent.set_property("visible", Variant::Bool(false));
    }
    let parent_id = tree.add_child(root, parent).unwrap();

    let mut child = Node::new("Child", "Sprite2D");
    child.set_property("position", Variant::Vector2(Vector2::new(50.0, 0.0)));
    let child_id = tree.add_child(parent_id, child).unwrap();

    (tree, child_id)
}

/// **The acceptance test.** Global transform, visibility, and z-order.
#[test]
fn editor_viewport_2d_global_transform_render() {
    // The visible and hidden scenes are identical apart from the parent's
    // `visible` flag, so every pixel that differs between them IS the child.
    // That difference is the child's footprint, located without ever guessing
    // what the background looks like.
    let (tree, _child) = rotated_parent_scene(true);
    let (hidden_tree, _) = rotated_parent_scene(false);
    let shown = render_scene_with_zoom_pan(&tree, None, W, H, 1.0, (0.0, 0.0));
    let hidden = render_scene_with_zoom_pan(&hidden_tree, None, W, H, 1.0, (0.0, 0.0));

    // -- (1) the child draws at its GLOBAL position -------------------------
    let (gx, gy) = scene_to_pixel(&tree, Vector2::new(100.0, 150.0));
    let (lx, ly) = scene_to_pixel(&tree, Vector2::new(150.0, 100.0));

    assert!(
        differs_near(&shown, &hidden, gx, gy, 8),
        "the sprite must be drawn at its global position (100,150) -> pixel \
         ({gx}, {gy}); nothing appears there"
    );
    assert!(
        !differs_near(&shown, &hidden, lx, ly, 4),
        "nothing may be drawn at (150,100) -> pixel ({lx}, {ly}) — that is the \
         position you get by summing local offsets and ignoring the parent's \
         90° rotation, i.e. the bug this bead fixes"
    );

    // -- (2) a hidden parent removes the child's pixels ---------------------
    //
    // Clause (1) already proves the two renders differ at the child's location;
    // this pins the direction — the hidden render is the one missing content,
    // and hiding the PARENT is what removed the CHILD.
    let solo = {
        // Same scene with no child at all: the hidden render must match it.
        let mut t = SceneTree::new();
        let root = t.root_id();
        let mut parent = node2d("Parent", Vector2::new(100.0, 100.0));
        parent.set_property("rotation", Variant::Float(std::f64::consts::FRAC_PI_2));
        parent.set_property("visible", Variant::Bool(false));
        t.add_child(root, parent).unwrap();
        render_scene_with_zoom_pan(&t, None, W, H, 1.0, (0.0, 0.0))
    };
    assert!(
        !differs_near(&hidden, &solo, gx, gy, 8),
        "a hidden parent must hide its whole subtree — the child still leaves \
         pixels at ({gx}, {gy}) that a childless scene does not"
    );

    // -- (3) z_index orders the draw ----------------------------------------
    //
    // Two same-size rects at one spot, differing only in which has the higher
    // z. Swapping the z values must swap which colour survives; if draw order
    // ignored z the two renders would be byte-identical.
    let stacked = |back_z: i64, front_z: i64| {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let at = Vector2::new(60.0, 60.0);
        for (name, zi, color) in [
            ("A", back_z, (1.0f32, 0.0f32, 0.0f32)),
            ("B", front_z, (0.0f32, 0.0f32, 1.0f32)),
        ] {
            let mut r = Node::new(name, "ColorRect");
            r.set_property("position", Variant::Vector2(at));
            r.set_property("size", Variant::Vector2(Vector2::new(40.0, 40.0)));
            r.set_property("z_index", Variant::Int(zi));
            r.set_property(
                "color",
                Variant::Color(gdcore::math::Color::new(color.0, color.1, color.2, 1.0)),
            );
            tree.add_child(root, r).unwrap();
        }
        let fb = render_scene_with_zoom_pan(&tree, None, W, H, 1.0, (0.0, 0.0));
        let p = scene_to_pixel(&tree, Vector2::new(at.x + 20.0, at.y + 20.0));
        (fb, p)
    };

    let (a_over_b, p) = stacked(1, 0);
    let (b_over_a, _) = stacked(0, 1);
    assert!(
        differs_near(&a_over_b, &b_over_a, p.0, p.1, 2),
        "swapping z_index must change which rect is on top at ({}, {}); \
         identical output means draw order ignored z entirely",
        p.0,
        p.1
    );
}

/// A node hidden by its OWN `visible` flag disappears too, not only one hidden
/// by an ancestor.
#[test]
fn self_hidden_node_is_not_drawn() {
    let shown = {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        tree.add_child(root, node2d("Solo", Vector2::new(120.0, 90.0)))
            .unwrap();
        render_scene_with_zoom_pan(&tree, None, W, H, 1.0, (0.0, 0.0))
    };
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = node2d("Solo", Vector2::new(120.0, 90.0));
    n.set_property("visible", Variant::Bool(false));
    tree.add_child(root, n).unwrap();
    let hidden = render_scene_with_zoom_pan(&tree, None, W, H, 1.0, (0.0, 0.0));

    let (x, y) = scene_to_pixel(&tree, Vector2::new(120.0, 90.0));
    assert!(
        differs_near(&shown, &hidden, x, y, 6),
        "a node with visible = false must not be drawn, so the two renders \
         must differ where it would have been"
    );
}

/// Selection hit-testing uses the same transform the renderer draws with, so a
/// click lands on the node whose pixels are actually there.
#[test]
fn hit_test_agrees_with_the_rendered_position() {
    use gdeditor::scene_renderer::hit_test_with_zoom_pan;

    let (tree, child) = rotated_parent_scene(true);
    let (gx, gy) = scene_to_pixel(&tree, Vector2::new(100.0, 150.0));

    let hit = hit_test_with_zoom_pan(&tree, W, H, 1.0, (0.0, 0.0), gx, gy);
    assert_eq!(
        hit,
        Some(child),
        "clicking where the sprite is DRAWN must select it; hit-testing against \
         local positions would answer None (or the wrong node) here"
    );
}

/// The scene bounds — which decide where the editor camera centres — are
/// computed from global positions too. Local bounds plus global drawing would
/// open a transform-heavy scene off-screen.
#[test]
fn scene_bounds_use_global_positions() {
    let (tree, _child) = rotated_parent_scene(true);
    let bounds = compute_scene_bounds(&tree);

    // The child's global position is (100,150); local summing would say
    // (150,100). The bounds must contain the former.
    assert!(
        bounds.position.y + bounds.size.y >= 150.0,
        "bounds must reach the child's global y of 150, got {bounds:?}"
    );
}
