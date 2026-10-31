//! pat-mgixk.45 acceptance — the editor's Play tick goes through ONE runtime
//! canvas entry point, so a Play frame gets CanvasGroups, generated noise,
//! normal-mapped lighting, the window content scale and the project's canvas
//! quality.
//!
//! # What was wrong
//!
//! There were THREE runtime canvas pipelines, each remembering a different
//! subset of the steps:
//!
//! | step | `render_runtime_canvas` | `scene_renderer::render_runtime_frame` | editor.rs Play |
//! |---|---|---|---|
//! | CanvasGroup compositing | yes | no | **no** |
//! | NoiseTexture2D bake | yes | no | **no** |
//! | normal/specular surfaces | yes | no | **no** |
//! | window stretch transform | no | yes | **no** |
//! | themed Control pass | no | no | yes |
//! | canvas quality (msaa/snap) | no | no | **no** |
//!
//! So `collect_canvas_groups`, `bake_noise_textures`, `canvas_surface_buffer`
//! and `CanvasQuality::from_project_settings` had NO production caller, and the
//! tests driving `render_runtime_canvas` — which say they exercise "the real
//! Play path" — were asserting against a path the Play tab did not run.
//!
//! # The path driven
//!
//! ```text
//! .tscn + project.godot
//!   -> render_play_frame   (the exact function examples/editor.rs calls per
//!                           Play tick, via gdeditor::viewport_2d_render)
//!     -> list + controls -> camera2d -> stretch -> render_canvas_from_list
//!        (groups, noise bake, surfaces, quality raster, light pass)
//! ```
//!
//! The fixture puts each feature in its own screen region so the assertions do
//! not read each other's pixels.

use gdcore::content_scale::{ContentScaleConfig, ContentScaleMode};
use gdcore::math::{Color, Vector2};
use gdeditor::texture_cache::TextureCache;
use gdeditor::viewport_2d_render::render_play_frame;
use gdobject::class_db::register_all_classes;
use gdrender2d::texture::Texture2D;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// Base resolution the project declares; the frame is rendered at 2x this.
const BASE: Vector2 = Vector2 { x: 320.0, y: 180.0 };
const W: u32 = 640;
const H: u32 = 360;

/// Canvas coordinates map to window pixels at exactly 2x, which is what makes
/// the stretch assertion a single concrete pixel.
const SCALE: f32 = 2.0;

/// ⚠️ The ColorRects use `position` + `size`, NOT Godot's
/// `offset_left`/`offset_top`. In this engine `control::get_position` and
/// `get_size` read the `"position"` and `"size"` properties only — the
/// `offset_*` family has setters/getters but nothing in the draw path derives a
/// rect from it. An earlier revision of this fixture used `offset_left = 10.0`
/// and every rect silently sat at (0, 0): the stretch assertion then read
/// background and reported a uniform grey. The existing fixtures elsewhere in
/// the tree all use `offset_left = 0.0`, which cannot tell the two spellings
/// apart.
///
/// Everything the acceptance names, laid out so no two features overlap:
///
/// * `Plate` — a ColorRect at canvas (10,10) size 10, for the stretch check.
/// * `Group` — a CanvasGroup with `self_modulate` alpha 0.5 over two
///   overlapping opaque ColorRects. ⚠️ `self_modulate`, not `modulate`:
///   children inherit `modulate` but NOT `self_modulate`, so this is the case
///   that distinguishes a flattened group from per-child blending.
/// * `Wall` + `Lamp` — a Sprite2D with a CanvasTexture normal map beside a
///   PointLight2D.
/// * `Snap` — the ONLY node whose origin lands on an exact HALF pixel, and the
///   whole reason clause 5 can observe anything.
///
///   🔑 Snapping is a no-op at almost every offset, and that is a property of
///   this rasterizer rather than a gap in it: `fill_rect_transformed`
///   (`gdrender2d/src/draw.rs`) samples at pixel CENTRES against a binary
///   inside/outside test, so an edge is always hard and there is no partial
///   coverage to move. The first covered column is `ceil(ox - 0.5)` unsnapped
///   and `floor(ox + 0.5)` snapped, and those agree for every `ox` **except an
///   exact half-integer**. `tests/canvas_quality.rs` documents this and pins it
///   in `snap_changes_the_raster_at_a_half_pixel_origin` (10.5 -> column 10
///   unsnapped, 11 snapped); that test is the reference this node copies.
///
///   So the fraction has to survive the 2x stretch AS a half: canvas
///   **160.25 -> final 320.5**. An earlier revision used 160.4, which stretches
///   to 320.8 — one of the offsets where the two formulas agree, so it provably
///   could not change a pixel, and clause 5 failed identically with it in place.
///   Every other node sits on a whole canvas pixel (10, 100, 110, 230, 60), so
///   without this one the snapped and unsnapped frames are identical however
///   correctly `CanvasQuality` is wired — which is what clause 5 was really
///   failing on, not the "settings were never read" its message suggests.
///
///   ⚠️ A ColorRect, not a Sprite2D: the rule above was measured on
///   `fill_rect_transformed`, the ColorRect path. The acceptance text names "a
///   Sprite2D at x=10.4", but 10.4 cannot work for the reason above and a
///   sprite blit is a different sampler, so this follows the proven path.
///   `y = 130` stays whole, so only the x axis moves.
///
///   Placed in EMPTY canvas space (x 160.25..180.25, y 130..150): clear of
///   `Plate` (10..20), the group rects (100..130), `Patch` (x <= 92), `Wall`
///   (x >= 198) and every sampled band, so it cannot perturb clauses 1-4; and it
///   is in both arms of clause 2's differential, so it cancels there.
/// * `Patch` — a Sprite2D with a NoiseTexture2D.
const SCENE: &str = r#"[gd_scene load_steps=5 format=3]

[ext_resource type="Texture2D" path="res://wall_d.png" id="1_d"]
[ext_resource type="Texture2D" path="res://wall_n.png" id="1_n"]

[sub_resource type="CanvasTexture" id="CanvasTexture_1"]
diffuse_texture = ExtResource("1_d")
normal_texture = ExtResource("1_n")

[sub_resource type="NoiseTexture2D" id="NoiseTexture2D_1"]
width = 64
height = 64

[node name="Root" type="Node2D"]

[node name="Plate" type="ColorRect" parent="."]
position = Vector2(10, 10)
size = Vector2(10, 10)
color = Color(1, 0, 0, 1)

[node name="Group" type="CanvasGroup" parent="."]
self_modulate = Color(1, 1, 1, 0.5)

[node name="A" type="ColorRect" parent="Group"]
position = Vector2(100, 20)
size = Vector2(20, 20)
color = Color(1, 1, 1, 1)

[node name="B" type="ColorRect" parent="Group"]
position = Vector2(110, 20)
size = Vector2(20, 20)
color = Color(1, 1, 1, 1)

[node name="Wall" type="Sprite2D" parent="."]
position = Vector2(230, 60)
texture = SubResource("CanvasTexture_1")

[node name="Lamp" type="PointLight2D" parent="."]
position = Vector2(180, 60)
height = 32.0
energy = 4.0

[node name="Patch" type="Sprite2D" parent="."]
position = Vector2(60, 130)
texture = SubResource("NoiseTexture2D_1")

[node name="Snap" type="ColorRect" parent="."]
position = Vector2(160.25, 130)
size = Vector2(20, 20)
color = Color(1, 1, 1, 1)
"#;

/// `window/stretch/mode=canvas_items` with the 320x180 base, and the snap flag
/// the quality pass reads.
fn project_godot(snap: bool) -> String {
    format!(
        "config_version=5\n\n\
         [display]\n\n\
         window/size/viewport_width=320\n\
         window/size/viewport_height=180\n\
         window/stretch/mode=\"canvas_items\"\n\n\
         [rendering]\n\n\
         2d/snap/snap_2d_transforms_to_pixel={snap}\n"
    )
}

fn settings(snap: bool) -> gdcore::ProjectSettings {
    gdcore::ProjectSettings::from_godot_format(&project_godot(snap)).expect("project.godot parses")
}

/// A plain white diffuse map, so every brightness difference comes from
/// lighting rather than from albedo.
fn diffuse() -> Texture2D {
    Texture2D::solid(64, 64, Color::new(1.0, 1.0, 1.0, 1.0))
}

/// Left half faces `-X`, right half `+X`, so a side light shades the two halves
/// differently.
fn hemispherical_normal() -> Texture2D {
    let mut tex = Texture2D::solid(64, 64, Color::new(0.5, 0.5, 1.0, 1.0));
    for y in 0..64 {
        for x in 0..64 {
            let r = if x < 32 { 0.0 } else { 1.0 };
            tex.pixels[(y * 64 + x) as usize] = Color::new(r, 0.5, 1.0, 1.0);
        }
    }
    tex
}

/// The control: every texel faces the viewer, so the surface is flat.
fn flat_normal() -> Texture2D {
    Texture2D::solid(64, 64, Color::new(0.5, 0.5, 1.0, 1.0))
}

/// Renders the fixture through the Play entry point.
/// Renders a Play frame.
///
/// `grouped` selects the CanvasGroup parent (`true`, the real fixture) or a
/// plain `Node2D` carrying the same 0.5 alpha as `modulate` (`false`).
///
/// 🔑 `modulate`, NOT `self_modulate`, in the ungrouped variant, and the choice
/// is the whole point of the comparison. `modulate` propagates down the chain,
/// so each child blends at 0.5 ON ITS OWN — which is exactly the per-child
/// blending a missing `collect_canvas_groups` produces. `self_modulate` does not
/// inherit, so the children would have drawn fully opaque and BOTH sample points
/// would have moved, destroying the differential.
fn play_frame(normal: Texture2D, snap: bool, grouped: bool) -> Vec<Color> {
    // ⚠️ REQUIRED, and its absence fails as an EMPTY frame rather than an error:
    // `collect_canvas_items` filters on `is_class("CanvasItem")`, which walks the
    // ClassDB chain. Unpopulated, a ColorRect is not a CanvasItem, the list comes
    // back empty and every assertion below compares clear to clear.
    register_all_classes();

    // One fixture, two spellings of the same parent: nothing else differs, so
    // any pixel that changes between the two renders changed because of the
    // grouping.
    let source = if grouped {
        SCENE.to_string()
    } else {
        SCENE.replace(
            "[node name=\"Group\" type=\"CanvasGroup\" parent=\".\"]\nself_modulate = Color(1, 1, 1, 0.5)",
            "[node name=\"Group\" type=\"Node2D\" parent=\".\"]\nmodulate = Color(1, 1, 1, 0.5)",
        )
    };
    if !grouped {
        // ⚠️ A silent no-op `replace` would hand back the GROUPED scene and the
        // differential below would compare a frame with itself — passing the
        // control and failing the overlap check for no reason. Assert the swap
        // landed.
        assert_ne!(
            source, SCENE,
            "the ungrouped variant must actually replace the CanvasGroup node; \
             the fixture text must have drifted from the pattern"
        );
    }
    let scene = PackedScene::from_tscn(&source).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    // The root window carries the content scale; `render_play_frame` reads it and
    // overrides only `size` with the render target's.
    tree.root_window_mut().config = ContentScaleConfig {
        mode: ContentScaleMode::CanvasItems,
        base_size: BASE,
        ..ContentScaleConfig::default()
    };

    let mut cache = TextureCache::new(".");
    cache.insert("res://wall_d.png", diffuse());
    cache.insert("res://wall_n.png", normal);

    render_play_frame(&mut tree, W, H, &mut cache, &settings(snap)).pixels
}

fn px(frame: &[Color], x: u32, y: u32) -> Color {
    frame[(y * W + x) as usize]
}

/// Canvas point -> the window pixel it lands on under the 2x stretch.
fn at(canvas_x: f32, canvas_y: f32) -> (u32, u32) {
    ((canvas_x * SCALE) as u32, (canvas_y * SCALE) as u32)
}

fn close(a: Color, b: Color, tol: f32) -> bool {
    (a.r - b.r).abs() < tol && (a.g - b.g).abs() < tol && (a.b - b.b).abs() < tol
}

#[test]
fn play_frame_uses_runtime_canvas_pipeline() {
    let frame = play_frame(hemispherical_normal(), true, true);
    assert_eq!(frame.len(), (W * H) as usize, "frame is 640x360");

    // -- (1) the window content scale reaches the Play frame ----------------
    //
    // The ColorRect occupies canvas (10,10)..(20,20). Unstretched that is window
    // (10,10)..(20,20) and pixel (25,25) is background; at the 2x the project's
    // 320x180 base implies in a 640x360 window it is window (20,20)..(40,40) and
    // (25,25) is inside it. Play never applied this transform at all.
    let (sx, sy) = at(12.5, 12.5);
    assert_eq!(
        (sx, sy),
        (25, 25),
        "fixture arithmetic: 12.5 canvas -> 25 window"
    );
    let plate = px(&frame, sx, sy);
    assert!(
        plate.r > 0.5 && plate.g < 0.5 && plate.b < 0.5,
        "a ColorRect at canvas (10,10) size 10 must cover window pixel (25,25) \
         under the project's canvas_items stretch — got {plate:?}",
    );
    // And the pixel just outside the stretched rect is NOT the rect, so the
    // assertion above is not passing on a frame flooded with red.
    let (ox, oy) = at(25.0, 25.0);
    assert!(
        !close(px(&frame, ox, oy), plate, 0.1),
        "canvas (25,25) is outside the 10x10 rect and must not be its colour",
    );

    // -- (2) CanvasGroup composites ONCE ------------------------------------
    //
    // Two opaque white children under a half-transparent group. Flattened, the
    // overlap is one 0.5 blend and matches the single-child area. Blended
    // per-child it is 1-0.5^2 = 0.75 alpha — a visibly brighter seam exactly
    // where they cross.
    // ⚠️ DIFFERENTIAL, not an absolute relationship between two regions of one
    // frame. The earlier version of this asserted `overlap == single`, reasoning
    // that a flattened group is one 0.5 blend while per-child blending is
    // 1-0.5^2 = 0.75. It failed three verifies, and the diagnosis (BlackWaterfall,
    // bead comment 08:02) is that the ASSERTION was wrong: a group's scratch
    // buffer is rasterized with a TRANSPARENT clear and then composited with the
    // group tint, which is not the same alpha math as a 0.5-alpha draw over the
    // OPAQUE runtime clear — so the single-child area was never the right oracle.
    // A PointLight2D also shades the composited frame, mixing lighting into both
    // samples.
    //
    // 🔑 Rendering the SAME fixture with the group replaced by a plain parent
    // removes both objections at once: the alpha math never has to be predicted,
    // and the lighting is common-mode and cancels.
    let ungrouped = play_frame(hemispherical_normal(), true, false);
    let (sx, sy) = at(105.0, 30.0);
    let (vx, vy) = at(115.0, 30.0);
    let single = px(&frame, sx, sy);
    let overlap = px(&frame, vx, vy);
    let single_ungrouped = px(&ungrouped, sx, sy);
    let overlap_ungrouped = px(&ungrouped, vx, vy);

    // Guard first: the children must actually be drawn, or everything below
    // compares background to background.
    let bg = px(&frame, at(160.0, 150.0).0, at(160.0, 150.0).1);
    assert!(
        !close(single, bg, 0.02),
        "precondition: the group's children must be drawn, or (2) compares \
         background to background (single {single:?} vs background {bg:?})",
    );

    // 🔑 The control. Only ONE child covers (105,30), so grouping cannot change
    // it: flattening one opaque rect and tinting it 0.5 is the same as drawing
    // that rect at 0.5. If this moves, the two renders differ for some reason
    // other than the grouping and the comparison below proves nothing.
    assert!(
        close(single, single_ungrouped, 0.02),
        "control: the single-child area must be identical with and without the \
         CanvasGroup — only one child covers it, so grouping cannot affect it. \
         grouped {single:?} vs ungrouped {single_ungrouped:?}",
    );

    // ...and the overlap, where BOTH children land, must change. Grouped it is
    // one composite of a flattened buffer; ungrouped it is two 0.5 blends
    // stacked. Equal values mean the group was never composited as a unit —
    // Play never ran collect_canvas_groups.
    assert!(
        !close(overlap, overlap_ungrouped, 0.02),
        "the CanvasGroup must composite as a UNIT: the overlap must differ from \
         the same scene drawn without the group ({overlap:?} vs \
         {overlap_ungrouped:?}), while the single-child area stays put. Equal \
         here means Play never ran collect_canvas_groups and the children \
         blended individually",
    );

    // -- (3) NoiseTexture2D is baked and sampled ----------------------------
    //
    // The sprite is 64x64 canvas units centred at (60,130); sample a band inside
    // it and require more than one distinct value. An unbaked generated texture
    // renders as a flat placeholder.
    let mut noise_values: Vec<Color> = Vec::new();
    for i in 0..24u32 {
        let (nx, ny) = at(40.0 + i as f32 * 1.5, 130.0);
        noise_values.push(px(&frame, nx, ny));
    }
    let distinct = noise_values
        .iter()
        .filter(|c| !close(**c, noise_values[0], 0.01))
        .count();
    assert!(
        distinct > 0,
        "NoiseTexture2D pixels must be non-uniform — Play never ran \
         bake_noise_textures, so the sprite drew flat; sampled {noise_values:?}",
    );

    // -- (4) the CanvasTexture normal map changes the lit pixels ------------
    let flat = play_frame(flat_normal(), true, true);
    let lit_differs = (0..64u32).any(|i| {
        let (lx, ly) = at(200.0 + i as f32, 60.0);
        !close(px(&frame, lx, ly), px(&flat, lx, ly), 0.01)
    });
    assert!(
        lit_differs,
        "a CanvasTexture normal map must change the lit sprite's pixels — Play \
         never ran canvas_surface_buffer, so the light shaded a flat surface",
    );

    // -- (5) the project's canvas quality reaches the rasterizer ------------
    //
    // Pixel snapping is the cheapest observable quality flag. Asserting that the
    // SETTING changes the frame, rather than a specific snapped coordinate,
    // keeps this from depending on whether the rounding lands in canvas or final
    // pixel space — what the bead needs proved is that
    // CanvasQuality::from_project_settings is wired at all, and it had no
    // production caller before.
    let unsnapped = play_frame(hemispherical_normal(), false, true);
    assert!(
        frame
            .iter()
            .zip(&unsnapped)
            .any(|(a, b)| !close(*a, *b, 0.004)),
        "snap_2d_transforms_to_pixel must change the Play frame — the quality \
         settings were never read on this path",
    );
}
