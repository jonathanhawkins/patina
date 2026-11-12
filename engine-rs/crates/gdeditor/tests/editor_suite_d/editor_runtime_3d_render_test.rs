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

use gdcore::math::Color;
use gdeditor::scene_renderer::{render_runtime_frame, render_scene_with_zoom_pan, scene_is_3d};
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
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
    tree.add_child(world, Node::new("Mesh", "MeshInstance3D"))
        .unwrap();
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
    t3.add_child(lvl, Node::new("M1", "MeshInstance3D"))
        .unwrap();
    t3.add_child(lvl, Node::new("M2", "MeshInstance3D"))
        .unwrap();
    assert!(scene_is_3d(&t3), "bare-Node root with 3D descendants is 3D");

    // Bare-`Node` root with 2D descendants → 2D.
    let mut t2 = SceneTree::new();
    let r = t2.root_id();
    let lvl = t2.add_child(r, Node::new("Level", "Node")).unwrap();
    t2.add_child(lvl, Node::new("S", "Sprite2D")).unwrap();
    assert!(
        !scene_is_3d(&t2),
        "bare-Node root with 2D descendants is 2D"
    );
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

// ---------------------------------------------------------------------------
// pat-mgixk.36: the Game view composites the canvas OVER the 3D world
// ---------------------------------------------------------------------------

/// Builds the acceptance fixture: a bare-`Node` root holding a `Camera3D` and a
/// `MeshInstance3D`, plus a `CanvasLayer` with **30** `ColorRect` children.
///
/// ⚠️ The 30 is load-bearing, not decoration. `scene_is_3d` falls back to
/// `count_3d > count_2d` for a bare-`Node` root, and 30 CanvasItems against 2
/// Node3Ds makes that comparison answer "2D" — so the old either/or renderer
/// drew the HUD and never drew the world at all. A fixture with fewer rects than
/// 3D nodes would take the 3D branch and the bug would be invisible.
fn hud_over_3d_scene() -> String {
    let mut tscn = String::from(
        r#"[gd_scene format=3]

[node name="Game" type="Node"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Mesh" type="MeshInstance3D" parent="."]
transform = Transform3D(4, 0, 0, 0, 4, 0, 0, 0, 4, 0, 0, 0)

[node name="Hud" type="CanvasLayer" parent="."]
"#,
    );
    // A 6x5 grid of 8x8 rects at a 12px pitch: 30 CanvasItems inside x 8..76,
    // y 8..56 of a 320x240 frame, so the whole HUD sits in a known band and
    // leaves most of the frame showing the 3D world.
    for i in 0..HUD_RECTS {
        let (col, row) = (i % 6, i / 6);
        let (x, y) = (8 + col * 12, 8 + row * 12);
        tscn.push_str(&format!(
            r#"
[node name="Rect{i}" type="ColorRect" parent="Hud"]
position = Vector2({x}, {y})
size = Vector2(8, 8)
color = Color(0, 1, 0, 1)
"#
        ));
    }
    tscn
}

/// How many `ColorRect`s the HUD holds — more than the scene's Node3D count on
/// purpose (see [`hud_over_3d_scene`]).
const HUD_RECTS: usize = 30;

/// The HUD's fill colour, chosen so no part of the 3D render can be mistaken for
/// it: the spatial renderer's background, grid and default mesh shades are all
/// grey/blue, never saturated green.
const HUD_COLOR: Color = Color::new(0.0, 1.0, 0.0, 1.0);

fn load_tscn(tscn: &str) -> SceneTree {
    // Register BEFORE parsing. Both halves matter here: without the 3D classes
    // `collect_scene_content` drops every spatial node, and without the 2D ones
    // there is no `ColorRect -> CanvasItem` chain so the canvas list comes back
    // empty — either way the frame would look like the composite failed when in
    // fact the fixture never populated ClassDB.
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn same_color(a: &Color, b: &Color) -> bool {
    let q = |v: f32| (v * 255.0).round() as i32;
    q(a.r) == q(b.r) && q(a.g) == q(b.g) && q(a.b) == q(b.b)
}

/// **The acceptance test (pat-mgixk.36).**
///
/// Godot's viewport renders its World3D and then composites the canvas layers
/// over it (`scene/main/viewport.cpp`). Patina chose ONE renderer, so a 3D game
/// with a HUD showed the HUD on an empty 2D clear colour and its world was never
/// drawn.
///
/// The assertions are written as a RELATION between two renders rather than as
/// absolute pixel counts, because the only question is whether the canvas was
/// composited over the 3D frame or replaced it:
///   * every pixel the HUD did not paint must still be exactly the 3D frame, and
///   * the pixels that changed must be the HUD's own.
///
/// Position-independent on purpose: it counts colours instead of sampling fixed
/// coordinates, so a project content-scale/stretch setting cannot silently make
/// it assert on the wrong pixels.
#[test]
fn play3d_composites_canvas_hud() {
    let tree = load_tscn(&hud_over_3d_scene());
    let (w, h) = (320, 240);

    // ⚠️ Grid and origin axes OFF, and the SAME viewport handed to both renders
    // below. Two reasons, and the first one is why this is not cosmetic: with
    // the grid on, "pixels that are not the background" counts grid lines and
    // the reddish origin axis, so the vacuity guard below would pass on editor
    // furniture alone and never notice a mesh that failed to draw. Second, a
    // Game-view frame has no editor furniture anyway, so this is also the more
    // faithful fixture.
    let mut vp = Viewport3D::new(w, h);
    vp.grid.visible = false;
    vp.show_origin = false;

    // The reference 3D-only frame: what the world looks like with no canvas.
    let pure_3d = render_editor_3d_frame(&vp, &tree);

    // GUARD AGAINST A VACUOUS TEST. Every assertion below compares against
    // `pure_3d`; if the fixture drew no 3D geometry at all, `pure_3d` would be a
    // flat background and "the 3D world survived the composite" would be
    // trivially true of two blank frames. With the furniture off, this many
    // non-background pixels can only be the MeshInstance3D — the "shows mesh
    // pixels" half of the acceptance.
    let bg = pure_3d.get_pixel(0, 0);
    let mesh_px = pure_3d
        .pixels
        .iter()
        .filter(|c| !same_color(c, &bg))
        .count();
    // Threshold kept well under the mesh's expected footprint (a 4x-scaled
    // placeholder cube) and well over incidental pixels, because the exact count
    // depends on the editor camera's default framing. A failure here is a FIXTURE
    // problem — the mesh did not draw or is off-screen — not a composite problem,
    // which is why it is asserted separately from everything below.
    assert!(
        mesh_px > 200,
        "fixture check: the 3D pass must actually draw the MeshInstance3D, else \
         the composite assertions below compare two blank frames (got {mesh_px} \
         non-background px)"
    );

    let frame = render_runtime_frame(&tree, &vp, w, h, 1.0, (0.0, 0.0));
    assert_eq!(
        (frame.width, frame.height),
        (w, h),
        "the Game-view frame is the requested size"
    );

    let hud_px = frame
        .pixels
        .iter()
        .filter(|c| same_color(c, &HUD_COLOR))
        .count();
    let changed: usize = frame
        .pixels
        .iter()
        .zip(pure_3d.pixels.iter())
        .filter(|(a, b)| !same_color(a, b))
        .count();

    // 1. The HUD reached the frame. This half ALREADY passed before the fix —
    //    the old code rendered the 2D canvas and nothing else — so it is not
    //    what makes this test red on HEAD.
    assert!(
        hud_px > 0,
        "the CanvasLayer's ColorRects must appear in the Game view"
    );
    // 30 rects of 8x8 is 1920 px of HUD; allow generous slack for edge
    // blending and any content scaling, but insist all 30 are not one rect.
    assert!(
        hud_px > 1000,
        "all {HUD_RECTS} ColorRects should be drawn, got only {hud_px} HUD px"
    );

    // 2. THE REGRESSION THIS BEAD FIXES: the 3D world is still there underneath.
    //    Before the fix `render_runtime_frame` returned the 2D canvas INSTEAD of
    //    the 3D frame, so essentially every pixel differed from `pure_3d` and
    //    this ratio collapsed.
    assert!(
        changed > 0,
        "the HUD must change something relative to the bare 3D frame"
    );
    let hud_share = hud_px as f64 / changed as f64;
    assert!(
        hud_share > 0.8,
        "the ONLY thing the canvas pass may change about the 3D frame is the \
         HUD's own pixels — {hud_px} of {changed} changed px are HUD-coloured \
         ({:.0}%). A low share means the 3D world was replaced by the canvas \
         rather than composited under it.",
        hud_share * 100.0
    );

    // 3. And state it the other way round, which is the part a ratio can hide:
    //    away from the HUD the frame is the 3D render, pixel for pixel.
    let non_hud_diff = frame
        .pixels
        .iter()
        .zip(pure_3d.pixels.iter())
        .filter(|(a, b)| !same_color(a, &HUD_COLOR) && !same_color(a, b))
        .count();
    assert!(
        non_hud_diff * 50 < frame.pixels.len(),
        "outside the HUD the Game view must BE the 3D frame; {non_hud_diff} of \
         {} px differ without being HUD-coloured",
        frame.pixels.len()
    );
}

/// The BGRA half of the composite (pat-mgixk.36).
///
/// ⚠️ This exists because of a GAP IN THE GATE, not for completeness. The Play
/// loop's GPU path lives in `engine-rs/examples/editor.rs`, and `--test
/// <target>` does not compile `[[example]]` targets — so that file's edit is not
/// covered by any command this bead can run. The fix for that was to keep the
/// only non-trivial logic OUT of the example: the example now just builds the
/// canvas list and delegates to `composite_canvas_over_bgra`, which is library
/// code and therefore testable right here.
///
/// The skip-on-transparent behaviour is the load-bearing part: it is what makes
/// a 3D scene with no CanvasItems publish byte-for-byte the frame it published
/// before this bead.
#[test]
fn canvas_composites_onto_a_bgra_frame_and_skips_transparent_pixels() {
    use gdrender2d::renderer::Bgra8Image;

    // Two opaque green pixels, in the BGRA byte order the GPU readback uses.
    let mut frame = Bgra8Image::from_bgra(2, 1, vec![0u8, 255, 0, 255, 0, 255, 0, 255]);

    // A canvas that paints the first pixel opaque red and leaves the second one
    // untouched — exactly what `rasterize_render_list` produces over a
    // transparent clear when a CanvasItem covers one pixel and not the other.
    let mut canvas = gdrender2d::renderer::FrameBuffer::new(2, 1, Color::new(0.0, 0.0, 0.0, 0.0));
    canvas.pixels[0] = Color::new(1.0, 0.0, 0.0, 1.0);

    gdeditor::scene_renderer::composite_canvas_over_bgra(&mut frame, &canvas);

    assert_eq!(
        &frame.data[0..4],
        &[0u8, 0, 255, 255],
        "the covered pixel takes the canvas colour (B, G, R, A)"
    );
    assert_eq!(
        &frame.data[4..8],
        &[0u8, 255, 0, 255],
        "the pixel the canvas did not paint must be left EXACTLY as the 3D pass \
         rendered it — this is what keeps a HUD-less 3D frame unchanged"
    );
}
