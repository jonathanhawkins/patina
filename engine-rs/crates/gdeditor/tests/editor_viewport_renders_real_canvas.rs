//! pat-mgixk.47 acceptance — the 2D editor viewport renders the edited scene
//! through the **runtime canvas list**, not a hand-written icon match.
//!
//! ## What was wrong
//!
//! `scene_renderer::render_scene_inner` drew class icons from a `match` on
//! `class_name`: only `Sprite2D` and `ColorRect` showed real content, `Label`
//! and `Button` were placeholder rects, and `Polygon2D`, `Line2D`,
//! `TileMapLayer`, `AnimatedSprite2D`, `NinePatchRect` and everything else fell
//! to `fill_circle(.., 2.0, COLOR_DEFAULT)` — a 2px dot. Nodes were placed by
//! global *position* alone, so **rotation, scale and modulate were ignored**:
//! the edit view and Play disagreed about the same scene.
//!
//! ## Why the assertions are coordinate-free
//!
//! The editor maps world to screen as `world * zoom + offset`, where `offset`
//! itself depends on `compute_scene_bounds(tree)` — so predicting an absolute
//! pixel means reimplementing the framing in the test, and any change to
//! centring would then break the test for the wrong reason.
//!
//! So each clause is phrased as a property of the *set* of matching pixels:
//!
//! * 🔑 **Rotation** — the triangle is a wide, shallow wedge rotated 90°. If
//!   rotation is applied its red footprint is **taller than it is wide**; if
//!   rotation is ignored (the old behaviour) the footprint stays wide and
//!   short. That is a one-bit answer with no coordinate arithmetic, and it
//!   fails for exactly the bug this bead names.
//! * **Line2D / Label** — their colours are distinctive, so "these pixels
//!   exist, and in quantity" separates a real draw from the old 2px dot.
//! * **Modulate alpha** — asserted by rendering the same scene twice, at
//!   modulate alpha 1.0 and 0.5, and comparing. A blend cannot be faked by an
//!   opaque draw, and no absolute coordinate is needed.

use gdcore::math::Color;
use gdrender2d::renderer::FrameBuffer;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 200;
const H: u32 = 200;

/// A wide, shallow wedge: 24 long on X, 6 tall on Y. Rotated a quarter turn it
/// becomes 6 wide and 24 tall, which is what the bbox assertion reads.
///
/// Deliberately NOT symmetric — an equilateral triangle looks the same rotated
/// and would make the rotation clause vacuous.
const TRI: &str = "PackedVector2Array(0, 0, 24, 0, 0, 6)";

fn scene(rect_alpha: f32) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Tri" type="Polygon2D" parent="."]
position = Vector2(60, 60)
rotation = 1.5707964
polygon = {TRI}
color = Color(1, 0, 0, 1)

[node name="Line" type="Line2D" parent="."]
position = Vector2(120, 40)
points = PackedVector2Array(0, 0, 0, 40)
width = 4.0
default_color = Color(0, 0, 1, 1)

[node name="Plate" type="Polygon2D" parent="."]
position = Vector2(130, 130)
polygon = PackedVector2Array(0, 0, 40, 0, 40, 40, 0, 40)
color = Color(1, 1, 1, 1)
modulate = Color(1, 1, 1, {rect_alpha})
"#
    )
}

/// Drives the exact entry the acceptance names, `render_scene_with_zoom_pan`.
///
/// An earlier revision used `render_scene_with_textures` so a `Label` could
/// rasterize glyphs. That clause is gone (the themed Control pass needs
/// `&mut SceneTree`; see clause 3), so there is no longer any reason to deviate
/// — and the texture cache was never the obstacle anyway:
/// `font_woff_label_render.rs:121` rasterizes glyphs with `None` for it.
fn render(rect_alpha: f32) -> FrameBuffer {
    gdobject::class_db::register_2d_classes();
    let packed = PackedScene::from_tscn(&scene(rect_alpha)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("scene instances");
    gdeditor::scene_renderer::render_scene_with_zoom_pan(&tree, None, W, H, 1.0, (0.0, 0.0))
}

fn pixel(fb: &FrameBuffer, x: u32, y: u32) -> Color {
    fb.pixels[(y * fb.width + x) as usize]
}

/// Every pixel satisfying `hit`, as `(x, y)`.
fn matching(fb: &FrameBuffer, hit: impl Fn(Color) -> bool) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for y in 0..fb.height {
        for x in 0..fb.width {
            if hit(pixel(fb, x, y)) {
                out.push((x, y));
            }
        }
    }
    out
}

/// `(width, height)` of the bounding box of `points`.
fn extent(points: &[(u32, u32)]) -> (u32, u32) {
    let xs: Vec<u32> = points.iter().map(|p| p.0).collect();
    let ys: Vec<u32> = points.iter().map(|p| p.1).collect();
    let w = xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1;
    let h = ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1;
    (w, h)
}

/// Strongly red: red channel dominant, the others low. Tolerant of the grid
/// showing through at the edges, strict enough not to catch the crosshair's
/// red X — which is 1px and cannot produce a 6x24 footprint.
fn is_red(c: Color) -> bool {
    c.r > 0.6 && c.g < 0.35 && c.b < 0.35
}

fn is_blue(c: Color) -> bool {
    c.b > 0.6 && c.r < 0.35 && c.g < 0.35
}

/// **The acceptance test.**
#[test]
fn editor_viewport_renders_real_canvas() {
    let fb = render(0.5);

    // -- 1. 🔑 the rotated Polygon2D, and that rotation was APPLIED ----------
    let red = matching(&fb, is_red);
    assert!(
        !red.is_empty(),
        "a Polygon2D must render as its filled polygon in the edit viewport; no \
         red pixels at all means it is still falling through to the 2px \
         COLOR_DEFAULT dot"
    );
    let (rw, rh) = extent(&red);
    assert!(
        rh > rw,
        "the wedge is 24 wide by 6 tall and carries rotation = PI/2, so its \
         footprint must come out TALLER than wide. Got {rw}x{rh}, which is the \
         unrotated shape — the viewport is placing nodes by global position and \
         ignoring the rest of the transform, the exact bug this bead names"
    );

    // -- 2. the Line2D draws as a line, not a dot ----------------------------
    let blue = matching(&fb, is_blue);
    assert!(
        blue.len() > 20,
        "a Line2D 40 long at width 4 must cover well over 20 pixels; got {} — a \
         handful means the old 2px placeholder dot",
        blue.len()
    );
    let (_, bh) = extent(&blue);
    assert!(
        bh > 10,
        "the Line2D runs 40 along Y, so its footprint must be tall; got height \
         {bh}"
    );

    // -- 3. (the acceptance's Label clause lives in the test below) ----------
    //
    // 🔑 `editor_viewport_renders_control_text_through_the_control_pass`
    // covers it, through `render_scene_with_zoom_pan_mut` — on a Button rather
    // than a Label, because the immutable path already draws Label text (see
    // that test's own note). This test stays on the
    // immutable entry on purpose: it is the one `render_scene` and every other
    // pre-existing wrapper funnel into, so keeping a clause here pins that the
    // Node2D-side canvas list did not regress when the Control pass was added.
    //
    // ⚠️ The alpha clause below also had to move off `ColorRect` for the same
    // reason. A ColorRect is a **Control**: its size comes from `offset_*`
    // resolved by the layout pass, which lives in
    // `build_control_render_items` — the `&mut SceneTree` call this viewport
    // cannot make. With no layout it is zero-sized and paints nothing, so the
    // first revision's baseline ("at alpha 1.0 the rect must be pure white")
    // failed with no white pixels at all. `canvas_composite.rs:34` warns about
    // exactly this and picks Polygon2D for it. Modulate is the thing under
    // test, not the node class, so the plate is a Node2D-based Polygon2D.
    //
    // What arrives through the IMMUTABLE path: the Node2D-side CanvasItems —
    // Polygon2D, Line2D, Sprite2D — which is what every clause here is built
    // from. Controls produce no items at all on it, which is why the plate
    // above is a Polygon2D rather than the acceptance's ColorRect.

    // -- 4. 🔑 modulate alpha blends over the grid ---------------------------
    //
    // Rendered twice and compared, so no absolute coordinate is needed and an
    // opaque draw cannot satisfy it: at alpha 1.0 the plate is pure white, at
    // 0.5 every one of those pixels must have moved toward the background.
    let opaque = render(1.0);
    let pure_white = matching(&opaque, |c| c.r > 0.95 && c.g > 0.95 && c.b > 0.95);
    assert!(
        !pure_white.is_empty(),
        "control: at modulate alpha 1.0 the plate must paint pure white pixels, \
         or the blend comparison below has no baseline"
    );
    let still_white_at_half = pure_white
        .iter()
        .filter(|&&(x, y)| {
            let c = pixel(&fb, x, y);
            c.r > 0.95 && c.g > 0.95 && c.b > 0.95
        })
        .count();
    assert_eq!(
        still_white_at_half,
        0,
        "the plate at modulate alpha 0.5 must BLEND over the grid: {} of {} \
         pixels that are pure white at alpha 1.0 are still pure white at 0.5, \
         so modulate is being ignored",
        still_white_at_half,
        pure_white.len()
    );
}

// ---------------------------------------------------------------------------
// Clause 3 of the acceptance: Control TEXT in the edit viewport.
// ---------------------------------------------------------------------------

/// A `Control` root with one positioned `Button`.
///
/// 🔑 A **Button**, not the acceptance's `Label`, and the reason is measured
/// rather than stylistic: `render_kind_for` already has a `"Label"` arm
/// (`canvas_render.rs:1747`) returning `Render2DKind::Text`, and `Label` is
/// registered with `.parent("Control")` inside `register_2d_classes`
/// (`class_db.rs:19388-19402`), so `collect_canvas_items`' `is_class("CanvasItem")`
/// gate admits it and the IMMUTABLE path draws Label text already. A Label
/// clause here would therefore pass without the Control pass — it would be
/// vacuous. `render_kind_for` has arms for only MeshInstance2D,
/// BackBufferCopy, MultiMeshInstance2D, Sprite2D/AnimatedSprite2D, Polygon2D,
/// Line2D, Path2D, TileMapLayer/TileMap, ColorRect, Label and the two particle
/// classes: **Button, Panel, TextureRect and RichTextLabel fall to `Marker`**,
/// so for those the themed Control pass is the only thing that can draw them.
///
/// ⚠️ `position`/`size`, NOT `offset_left`/`offset_top`. Nothing in the control
/// draw path reads the offsets — `control::get_size` reads `"size"` only
/// (`control.rs`), and `default_theme_text_renders_ttf_in_play_view.rs:105`
/// documents the same trap. Authoring the rect directly also keeps the clause
/// independent of `sort_container_tree`: there is no container here, so the
/// geometry under test is the Button's own.
fn button_scene(text: &str) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Control"]

[node name="Press" type="Button" parent="."]
position = Vector2(40, 80)
size = Vector2(120, 40)
text = "{text}"
"#
    )
}

/// Renders `button_scene` through either editor entry.
///
/// `controls = false` is the pre-existing immutable wrapper — the behaviour at
/// HEAD. `controls = true` is the new one. Both are driven here so the clause
/// below is a DIFFERENTIAL rather than an absolute claim about one render.
fn render_button(text: &str, controls: bool) -> FrameBuffer {
    gdobject::class_db::register_2d_classes();
    let packed = PackedScene::from_tscn(&button_scene(text)).expect("button scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("button scene instances");
    if controls {
        gdeditor::scene_renderer::render_scene_with_zoom_pan_mut(
            &mut tree,
            None,
            W,
            H,
            1.0,
            (0.0, 0.0),
        )
    } else {
        gdeditor::scene_renderer::render_scene_with_zoom_pan(&tree, None, W, H, 1.0, (0.0, 0.0))
    }
}

/// How many pixels differ between two frames.
///
/// 🔑 Deliberately COLOUR-FREE. The first revision of this clause keyed on a
/// `theme_override_colors/font_color` and could never pass:
/// `Render2DKind::Text` (`canvas_render.rs:259-270`) carries text, font_size,
/// h_align and font_path and **no colour at all**, so a font-colour override
/// cannot reach the pixels through this pipeline no matter which pass emits
/// the item. Asking only "did the pixels change" avoids encoding any claim
/// about what colour the engine picks.
fn differing(a: &FrameBuffer, b: &FrameBuffer) -> usize {
    let mut n = 0;
    for i in 0..(a.width * a.height) as usize {
        let (p, q) = (a.pixels[i], b.pixels[i]);
        if (p.r - q.r).abs() > 0.01 || (p.g - q.g).abs() > 0.01 || (p.b - q.b).abs() > 0.01 {
            n += 1;
        }
    }
    n
}

/// **The acceptance's Control-text clause.** A Control's TEXT reaches the 2D
/// edit viewport, and it is the themed Control pass that puts it there.
#[test]
fn editor_viewport_renders_control_text_through_the_control_pass() {
    // Emptying the string is the ONE input that changes between the two frames
    // of each arm. The placeholder rect and the themed stylebox are both sized
    // from `size`, which does not move, so whatever differs is the GLYPHS.
    let with_controls = differing(&render_button("Hi", true), &render_button("", true));
    let immutable = differing(&render_button("Hi", false), &render_button("", false));

    // -- 1. the glyphs are there --------------------------------------------
    assert!(
        with_controls > 10,
        "a Button's text must rasterize in the 2D edit viewport: emptying the          string changed only {with_controls} pixels, so either the themed          Control pass is not reaching the editor frame or the opaque          COLOR_CONTROL placeholder is still being painted over it"
    );

    // -- 2. 🔑 it is the Control pass that delivers it ------------------------
    //
    // `render_kind_for` has no Button arm, so on the immutable path the node
    // falls to `Marker` and the class match draws `draw_button_rect` — sized
    // from `size`, with the text never read. Emptying the string must therefore
    // change NOTHING there. That is what makes clause 1 a statement about the
    // new path rather than about the fixture.
    assert_eq!(
        immutable, 0,
        "the immutable entry cannot build Control render items and has no          Button arm in `render_kind_for`, so its frame must be byte-identical          whatever the Button's text says; {immutable} pixels moved, which means          this clause is not measuring the Control pass and would pass without it"
    );
}
