//! Regression test: pressing Play on a **3D** scene must render the running
//! scene in the Game view, not a blank canvas.
//!
//! The bug the user hit: they opened a 3D scene (`outdoor_3d.tscn` — `Node3D`,
//! `MeshInstance3D`, `Camera3D`, lights), pressed Play, the editor switched to
//! the Game view (correct) but showed an EMPTY grid — "I hit play and I don't
//! see the rendering". Root cause: the editor's run loop always rendered the
//! running scene with the 2D canvas rasterizer (`render_scene_with_zoom_pan`),
//! which has nothing to draw for a 3D scene (no `CanvasItem`s), so the frame was
//! effectively blank.
//!
//! The fix: `scene_renderer::render_runtime_frame` chooses the renderer from the
//! scene type — Godot's main-plugin selection rule (`_find_node_types`) — so a
//! 3D scene is drawn through the spatial renderer and a 2D scene through the
//! canvas rasterizer. These tests lock that in at the unit level (the live
//! browser behavior is verified separately).

use gdeditor::scene_renderer::{render_runtime_frame, render_scene_with_zoom_pan, scene_is_3d};
use gdeditor::viewport_3d::Viewport3D;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::SceneTree;

fn register_classes() {
    gdobject::class_db::register_2d_classes();
    gdobject::class_db::register_3d_classes();
}

/// Number of pixels whose color differs from the frame's background (top-left
/// corner) — a proxy for "how much was actually drawn".
fn content_pixels(fb: &FrameBuffer) -> usize {
    let bg = fb.get_pixel(0, 0);
    let q = |c: &gdcore::math::Color| {
        (
            (c.r * 255.0) as i32,
            (c.g * 255.0) as i32,
            (c.b * 255.0) as i32,
        )
    };
    let bgq = q(&bg);
    fb.pixels.iter().filter(|c| q(c) != bgq).count()
}

/// A 3D scene: `Node3D` root with a `MeshInstance3D` child (renders a mesh box).
fn scene_3d() -> SceneTree {
    register_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let world = tree.add_child(root, Node::new("World", "Node3D")).unwrap();
    tree.add_child(world, Node::new("Mesh", "MeshInstance3D")).unwrap();
    tree
}

/// A 2D scene: `Node2D` root with a `Node2D` child.
fn scene_2d() -> SceneTree {
    register_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = tree.add_child(root, Node::new("Main", "Node2D")).unwrap();
    tree.add_child(main, Node::new("Child", "Node2D")).unwrap();
    tree
}

/// `scene_is_3d` follows Godot's main-plugin selection: the scene root's own type
/// wins, and a bare-`Node` root falls back to whichever spatial/canvas node type
/// dominates the descendants.
#[test]
fn scene_is_3d_classifies_by_godot_main_plugin_rule() {
    register_classes();

    assert!(scene_is_3d(&scene_3d()), "Node3D-rooted scene is 3D");
    assert!(!scene_is_3d(&scene_2d()), "Node2D-rooted scene is 2D");

    // Bare-`Node` root with 3D descendants → 3D.
    let mut t3 = SceneTree::new();
    let r = t3.root_id();
    let lvl = t3.add_child(r, Node::new("Level", "Node")).unwrap();
    t3.add_child(lvl, Node::new("M1", "MeshInstance3D")).unwrap();
    t3.add_child(lvl, Node::new("M2", "MeshInstance3D")).unwrap();
    assert!(scene_is_3d(&t3), "bare-Node root with 3D descendants is 3D");

    // Bare-`Node` root with 2D descendants → 2D.
    let mut t2 = SceneTree::new();
    let r = t2.root_id();
    let lvl = t2.add_child(r, Node::new("Level", "Node")).unwrap();
    t2.add_child(lvl, Node::new("S", "Sprite2D")).unwrap();
    assert!(!scene_is_3d(&t2), "bare-Node root with 2D descendants is 2D");
}

/// The core regression: a running 3D scene must render real 3D content in the
/// Game view, NOT the blank 2D rasterization the run loop used to produce.
#[test]
fn runtime_3d_scene_renders_content_not_blank() {
    let tree = scene_3d();
    let vp = Viewport3D::new(320, 240);
    let (w, h) = (320, 240);

    // The OLD (buggy) path: the 2D canvas rasterizer on a 3D scene.
    let two_d = render_scene_with_zoom_pan(&tree, None, w, h, 1.0, (0.0, 0.0));
    // The FIXED path: render_runtime_frame routes a 3D scene to the spatial renderer.
    let runtime = render_runtime_frame(&tree, &vp, w, h, 1.0, (0.0, 0.0));

    assert_eq!(
        (runtime.width, runtime.height),
        (w, h),
        "the Game-view frame is the requested size"
    );

    // The fix changed the render path: a 3D scene no longer rasterizes as the 2D
    // (essentially blank) frame.
    assert!(
        runtime.pixels != two_d.pixels,
        "a running 3D scene must NOT render as the blank 2D rasterization"
    );

    // And the 3D render actually drew the scene: substantially more was painted
    // than the 2D rasterizer drew for the same 3D scene (which had no CanvasItems
    // to draw — only the empty grid chrome).
    let c_runtime = content_pixels(&runtime);
    let c_2d = content_pixels(&two_d);
    assert!(
        c_runtime > c_2d,
        "the 3D Game view paints real geometry (runtime={c_runtime} px) where the \
         2D rasterizer painted almost nothing for the 3D scene ({c_2d} px)"
    );
    assert!(
        c_runtime > 500,
        "the 3D render fills the viewport with the scene, got {c_runtime} content px"
    );
}

/// A 2D scene's Game view must use the 2D **canvas rasterizer** — the runtime
/// pixel path (pat-u3l8c.23) that draws the real CanvasItem pixels the game
/// shows — NOT the 3D spatial renderer. `render_runtime_frame` therefore
/// delegates a 2D scene to `render_runtime_canvas` (real pixels, no editor
/// grid/rulers/gizmos), which is distinct from the editor viewport's
/// class-glyph preview (`render_scene_with_zoom_pan`).
#[test]
fn runtime_2d_scene_uses_canvas_rasterizer() {
    use gdeditor::viewport_2d_render::{render_runtime_canvas, RUNTIME_CLEAR_COLOR};

    let tree = scene_2d();
    let vp = Viewport3D::new(320, 240);
    let (w, h) = (320, 240);

    let runtime = render_runtime_frame(&tree, &vp, w, h, 1.0, (0.0, 0.0));
    let canvas = render_runtime_canvas(&tree, w, h, RUNTIME_CLEAR_COLOR, None);

    assert_eq!(
        (runtime.width, runtime.height),
        (w, h),
        "the Game-view frame is the requested size"
    );
    assert_eq!(
        runtime.pixels, canvas.pixels,
        "a 2D scene's Game view renders through the runtime canvas rasterizer"
    );

    // The runtime path is the real-pixel rasterizer, NOT the editor viewport's
    // grid/ruler/marker preview — the two must differ.
    let editor_preview = render_scene_with_zoom_pan(&tree, None, w, h, 1.0, (0.0, 0.0));
    assert!(
        runtime.pixels != editor_preview.pixels,
        "the running Game view is not the editor's class-glyph/grid preview"
    );
}
