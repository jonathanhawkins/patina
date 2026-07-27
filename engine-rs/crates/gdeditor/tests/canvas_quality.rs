//! pat-0q2kx.254 — viewport canvas quality in the software raster:
//! `snap_2d_*_to_pixel`, `visibility_layer` × `canvas_cull_mask`, and `msaa_2d`.
//!
//! Godot ground truth: `Viewport.snap_2d_transforms_to_pixel`,
//! `snap_2d_vertices_to_pixel`, `canvas_cull_mask`, `msaa_2d` in
//! `extension_api.json`; the snap itself in
//! `servers/rendering/renderer_canvas_cull.cpp`.
//!
//! ⚠️ **The bead's snap example cannot be reproduced, and this is a fact about
//! the rasterizer, not a gap in the implementation.** The acceptance asks that
//! a ColorRect at `(10.6, 10.4)` snap to "pixel column 11 (no half-covered
//! blend column)" and that snapping *off* "reproduces the blended edge". There
//! is no blended edge to reproduce: `fill_rect_transformed`
//! (`gdrender2d/src/draw.rs:53`) samples at **pixel centres** — `inv.xform(px +
//! 0.5, py + 0.5)` against a binary inside/outside test — so a rect edge is
//! always hard. Nothing in this raster produces partial coverage.
//!
//! Worked through, that makes translation snapping a **no-op for the raster at
//! almost every offset**. The first covered column is `ceil(ox - 0.5)`
//! unsnapped and `floor(ox + 0.5)` snapped, and those two agree for every `ox`
//! except an exact half-integer. At `ox = 10.6` both give column 11 — the
//! bead's own example is one where snapping provably changes nothing.
//!
//! So this file proves the feature the two ways that are real:
//! * the transform is genuinely snapped (`snapped_transform_lands_on_whole_pixels`);
//! * and at the one offset where the raster can differ, it does
//!   (`snap_changes_the_raster_at_a_half_pixel_origin`).
//!
//! Reported to the coordinator rather than papered over — an assertion written
//! to the bead's wording would have had to be vacuous to pass.
//!
//! ⚠️ Top-level binary, not a module in an `editor_suite_*` group: the
//! verifier truncates a reported test command at 58 chars, and the scoped
//! `--test editor_suite_x -E 'test(/^…/)'` form does not fit, so a grouped test
//! can only be reported unscoped — which fails the bead whenever anything else
//! in the suite is red. `--test canvas_quality` is 51 chars and scopes. See
//! `tests/midi_e2e.rs` for the incident that established this.

use gdcore::math::{Color, Transform2D, Vector2};
use gdeditor::viewport_2d_render::{
    collect_canvas_items, rasterize_render_list_with_quality, CanvasFollow, CanvasQuality,
    Render2DItem, Render2DKind,
};
use gdrender2d::renderer::FrameBuffer;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 48;
const H: u32 = 48;
const CLEAR: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn load(tscn: &str) -> SceneTree {
    // ⚠️ REQUIRED, and its absence is silent. `ClassDB::new()` starts **empty**
    // (`gdobject/src/class_db.rs:271`) and nothing seeds it lazily, so without
    // this call `Node::is_class("CanvasItem")` is false for every node —
    // `collect_canvas_items` filters the entire scene out and returns `[]`.
    //
    // That is not a hypothetical: this test failed verification exactly that
    // way ("must reach the render item; got []"), which reads like the property
    // was not carried rather than like the scene was never collected. Any test
    // that loads a `.tscn` and expects nodes to have a class chain needs it.
    gdobject::class_db::register_2d_classes();

    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// A red ColorRect at `origin`, sized 20×20, on the default visibility layer.
fn rect_item(origin: Vector2) -> Render2DItem {
    Render2DItem {
        node_id: 1,
        global_transform: Transform2D {
            origin,
            ..Transform2D::IDENTITY
        },
        modulate: Color::new(1.0, 1.0, 1.0, 1.0),
        z_index: 0,
        canvas_layer: 0,
        clip_rect: None,
        light_mask: 1,
        visibility_layer: 1,
        canvas_follow: CanvasFollow::DefaultCanvas,
        kind: Render2DKind::ColorRect {
            size: Vector2::new(20.0, 20.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        },
    }
}

/// A 1px-wide diagonal Line2D — the shape whose edge MSAA is supposed to soften.
fn diagonal_line_item() -> Render2DItem {
    Render2DItem {
        kind: Render2DKind::Line {
            points: vec![Vector2::new(4.0, 4.0), Vector2::new(40.0, 40.0)],
            width: 1.0,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
        },
        ..rect_item(Vector2::new(0.0, 0.0))
    }
}

fn render(items: &[Render2DItem], quality: &CanvasQuality) -> FrameBuffer {
    rasterize_render_list_with_quality(items, W, H, CLEAR, None, quality)
}

/// Index of the first column on row `y` whose pixel is not the clear colour.
fn first_filled_column(fb: &FrameBuffer, y: u32) -> Option<u32> {
    (0..fb.width).find(|&x| fb.get_pixel(x, y).r > 0.01)
}

fn any_pixel_lit(fb: &FrameBuffer) -> bool {
    (0..fb.height).any(|y| (0..fb.width).any(|x| fb.get_pixel(x, y).r > 0.01))
}

/// **The acceptance.** The three clauses the bead names, with the snap clause
/// stated as what this rasterizer can actually show (see the module note).
#[test]
fn canvas_pixel_snap_and_msaa() {
    // -- (1) snapping puts the transform on whole pixels ---------------------
    let on = CanvasQuality {
        snap_2d_transforms_to_pixel: true,
        ..CanvasQuality::default()
    };
    let snapped = render(&[rect_item(Vector2::new(10.6, 10.4))], &on);
    let unsnapped = render(
        &[rect_item(Vector2::new(10.6, 10.4))],
        &CanvasQuality::default(),
    );
    // Column 11 either way — `ceil(10.6 - 0.5)` and `floor(10.6 + 0.5)` agree.
    // Asserted rather than skipped so that a future antialiased rasterizer,
    // which WOULD differ here, fails this and forces the comparison to be
    // rewritten deliberately.
    assert_eq!(
        first_filled_column(&snapped, 12),
        Some(11),
        "snapped: the rect's left edge is at column 11"
    );
    assert_eq!(
        first_filled_column(&unsnapped, 12),
        first_filled_column(&snapped, 12),
        "with pixel-centre sampling and no coverage AA, a 0.6px offset lands on \
         the same column snapped or not — the bead's example cannot distinguish \
         them; see `snap_changes_the_raster_at_a_half_pixel_origin`"
    );

    // -- (2) visibility_layer ANDed against canvas_cull_mask -----------------
    let layer2 = Render2DItem {
        visibility_layer: 2,
        ..rect_item(Vector2::new(10.0, 10.0))
    };
    let hidden = render(
        &[layer2.clone()],
        &CanvasQuality {
            canvas_cull_mask: 1,
            ..CanvasQuality::default()
        },
    );
    assert!(
        !any_pixel_lit(&hidden),
        "visibility_layer 2 must be culled by canvas_cull_mask 1 (2 & 1 == 0)"
    );
    let shown = render(
        &[layer2],
        &CanvasQuality {
            canvas_cull_mask: 3,
            ..CanvasQuality::default()
        },
    );
    assert!(
        any_pixel_lit(&shown),
        "canvas_cull_mask 3 includes layer 2 (2 & 3 == 2), so it must draw"
    );

    // -- (3) msaa_2d=4x softens a diagonal edge ------------------------------
    let aliased = render(&[diagonal_line_item()], &CanvasQuality::default());
    let msaa = render(
        &[diagonal_line_item()],
        &CanvasQuality {
            msaa_2d_factor: 4,
            ..CanvasQuality::default()
        },
    );
    let intermediate = |fb: &FrameBuffer| {
        (0..fb.height)
            .flat_map(|y| (0..fb.width).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let p = fb.get_pixel(x, y);
                p.r > 0.05 && p.r < 0.95
            })
            .count()
    };
    assert_eq!(
        intermediate(&aliased),
        0,
        "without MSAA every pixel of the line is fully in or fully out — the \
         raster does binary pixel-centre coverage"
    );
    assert!(
        intermediate(&msaa) > 0,
        "msaa_2d=4x must produce intermediate-alpha pixels along the diagonal; \
         got none, so the supersample or the downsample is not running"
    );
}

/// The transform really is rounded, independently of what the raster then does
/// with it. This is the assertion that would still hold under a different
/// rasterizer, so it is the durable statement of the feature.
#[test]
fn snapped_transform_lands_on_whole_pixels() {
    let on = CanvasQuality {
        snap_2d_transforms_to_pixel: true,
        ..CanvasQuality::default()
    };
    // Rendering is the only public path that applies the snap, so the check is
    // indirect: place the rect so that snapping is observable, at a half-pixel.
    for (ox, want_col) in [(10.5f32, 11u32), (11.5, 12), (9.5, 10)] {
        let fb = render(&[rect_item(Vector2::new(ox, 10.0))], &on);
        assert_eq!(
            first_filled_column(&fb, 12),
            Some(want_col),
            "origin {ox} must snap to {want_col} (floor(x + 0.5))"
        );
    }
}

/// The one offset where snapping provably changes the raster: an exact
/// half-pixel, where `ceil(ox - 0.5)` and `floor(ox + 0.5)` disagree.
///
/// Without this case the snap feature would be untested at the pixel level —
/// every other offset renders identically either way, so a completely broken
/// snap would still pass a comparison at, say, 10.6.
#[test]
fn snap_changes_the_raster_at_a_half_pixel_origin() {
    let off = render(
        &[rect_item(Vector2::new(10.5, 10.0))],
        &CanvasQuality::default(),
    );
    let on = render(
        &[rect_item(Vector2::new(10.5, 10.0))],
        &CanvasQuality {
            snap_2d_transforms_to_pixel: true,
            ..CanvasQuality::default()
        },
    );
    assert_eq!(
        first_filled_column(&off, 12),
        Some(10),
        "unsnapped 10.5 covers from column 10 (pixel centre 10.5 >= 10.5)"
    );
    assert_eq!(
        first_filled_column(&on, 12),
        Some(11),
        "snapped to 11, so column 10's centre (10.5) now falls outside"
    );
}

/// ⚠️ A cull mask of 0 hides everything and must stay that way. Treating it as
/// "unset" and substituting the all-layers default would make a deliberately
/// blanked viewport render — a plausible-looking "fix" that silently disagrees
/// with Godot.
#[test]
fn a_zero_cull_mask_hides_everything() {
    let fb = render(
        &[rect_item(Vector2::new(10.0, 10.0))],
        &CanvasQuality {
            canvas_cull_mask: 0,
            ..CanvasQuality::default()
        },
    );
    assert!(!any_pixel_lit(&fb), "cull mask 0 must draw nothing");
}

/// Godot's viewport default shows every layer, not just layer 1 — a viewport
/// displays everything until you narrow it. Defaulting to 1 would hide every
/// item an author had put on a higher layer.
#[test]
fn the_default_mask_shows_every_layer() {
    let q = CanvasQuality::default();
    assert_eq!(q.canvas_cull_mask, u32::MAX);
    for layer in [1u32, 2, 4, 0x8000_0000] {
        let item = Render2DItem {
            visibility_layer: layer,
            ..rect_item(Vector2::new(10.0, 10.0))
        };
        assert!(
            any_pixel_lit(&render(&[item], &q)),
            "layer {layer} must be visible under the default mask"
        );
    }
}

/// `msaa_2d` values Godot does not offer round **down** to a legal factor.
/// Rounding up would let a stray 5 cost a 64× allocation.
#[test]
fn msaa_factor_clamps_to_godots_enum() {
    let f = |n: u32| {
        CanvasQuality {
            msaa_2d_factor: n,
            ..CanvasQuality::default()
        }
        .msaa_factor()
    };
    assert_eq!((f(0), f(1)), (1, 1), "disabled");
    assert_eq!((f(2), f(3)), (2, 2));
    assert_eq!((f(4), f(7)), (4, 4));
    assert_eq!((f(8), f(999)), (8, 8), "clamped, never grown");
}

/// MSAA must not move anything: supersampling scales the basis as well as the
/// origin, so a rect lands on the same pixels with MSAA on as off.
///
/// Scaling only the origin — the easy mistake — would draw every item at its
/// original size in a corner-anchored cluster, which reads as "the scene
/// shrank" rather than as a transform bug.
#[test]
fn msaa_does_not_move_or_resize_items() {
    let plain = render(
        &[rect_item(Vector2::new(10.0, 10.0))],
        &CanvasQuality::default(),
    );
    let msaa = render(
        &[rect_item(Vector2::new(10.0, 10.0))],
        &CanvasQuality {
            msaa_2d_factor: 4,
            ..CanvasQuality::default()
        },
    );
    assert_eq!(
        first_filled_column(&plain, 12),
        first_filled_column(&msaa, 12),
        "the rect's left edge must not move when MSAA is enabled"
    );
    assert_eq!(
        (msaa.width, msaa.height),
        (W, H),
        "downsampled back to size"
    );
}

/// `visibility_layer` is read off the node, not defaulted — the property was
/// inspector-settable long before anything read it, so a scene could carry a
/// considered value that changed nothing.
#[test]
fn visibility_layer_is_read_from_the_scene() {
    const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="OnLayer2" type="ColorRect" parent="."]
position = Vector2(10, 10)
size = Vector2(8, 8)
visibility_layer = 2

[node name="Default" type="ColorRect" parent="."]
position = Vector2(20, 20)
size = Vector2(8, 8)
"#;
    let tree = load(SCENE);
    let items = collect_canvas_items(&tree);
    // Separated from the layer assertions on purpose: an empty list means the
    // scene was never collected (an unseeded ClassDB, a parse failure), which is
    // a completely different bug from "the property did not carry". Folding the
    // two together is what made the first failure here read misleadingly.
    assert!(
        !items.is_empty(),
        "no canvas items were collected at all — the scene did not load, or the \
         ClassDB was not seeded, so nothing below this can be meaningful"
    );
    let layers: Vec<u32> = items.iter().map(|i| i.visibility_layer).collect();
    assert!(
        layers.contains(&2),
        "the authored visibility_layer = 2 must reach the render item; got {layers:?}"
    );
    assert!(
        layers.contains(&1),
        "a node without the property must default to layer 1; got {layers:?}"
    );
}

/// ⚠️ **The MSAA project setting is an enum INDEX, not a factor.** Godot stores
/// `MSAA_DISABLED=0, MSAA_2X=1, MSAA_4X=2, MSAA_8X=3` and casts it straight to
/// the enum (`scene_tree.cpp:2079`), so a project asking for 4× writes `2`.
/// Reading that value as a factor would silently render at 2× — correct-looking
/// output at the wrong quality, which no assertion about "MSAA works" catches.
#[test]
fn msaa_project_setting_is_an_enum_index_not_a_factor() {
    use gdcore::project_settings::{ProjectSettings, SettingsValue};

    let mut ps = ProjectSettings::default();
    ps.set(
        "rendering/anti_aliasing/quality/msaa_2d",
        SettingsValue::Int(2),
    );
    assert_eq!(
        CanvasQuality::from_project_settings(&ps).msaa_2d_factor,
        4,
        "MSAA_4X is index 2, so the factor must be 4 — not 2"
    );

    for (index, factor) in [(0i64, 1u32), (1, 2), (2, 4), (3, 8)] {
        let mut ps = ProjectSettings::default();
        ps.set(
            "rendering/anti_aliasing/quality/msaa_2d",
            SettingsValue::Int(index),
        );
        assert_eq!(
            CanvasQuality::from_project_settings(&ps).msaa_2d_factor,
            factor,
            "MSAA enum index {index}"
        );
    }
    // Out of range falls back to disabled, never up.
    let mut ps = ProjectSettings::default();
    ps.set(
        "rendering/anti_aliasing/quality/msaa_2d",
        SettingsValue::Int(9),
    );
    assert_eq!(CanvasQuality::from_project_settings(&ps).msaa_2d_factor, 1);
}

/// The snap flags come off `project.godot` under Godot's real keys, and an
/// empty project gets Godot's defaults.
#[test]
fn project_settings_supply_the_canvas_quality() {
    use gdcore::project_settings::{ProjectSettings, SettingsValue};

    let empty = CanvasQuality::from_project_settings(&ProjectSettings::default());
    assert!(!empty.snap_2d_transforms_to_pixel, "Godot default is false");
    assert!(!empty.snap_2d_vertices_to_pixel);
    assert_eq!(empty.msaa_2d_factor, 1, "MSAA off by default");
    assert_eq!(empty.canvas_cull_mask, u32::MAX);
    // Godot's project defaults are index 1 for both SDF settings.
    assert_eq!(empty.sdf_oversize, 1.2, "index 1 is 120%");
    assert_eq!(empty.sdf_scale, 0.5, "index 1 is 50%");

    let mut ps = ProjectSettings::default();
    ps.set(
        "rendering/2d/snap/snap_2d_transforms_to_pixel",
        SettingsValue::Bool(true),
    );
    ps.set(
        "rendering/2d/snap/snap_2d_vertices_to_pixel",
        SettingsValue::Bool(true),
    );
    ps.set("rendering/2d/sdf/oversize", SettingsValue::Int(3));
    ps.set("rendering/2d/sdf/scale", SettingsValue::Int(2));
    let q = CanvasQuality::from_project_settings(&ps);
    assert!(q.snap_2d_transforms_to_pixel);
    assert!(q.snap_2d_vertices_to_pixel);
    assert_eq!(q.sdf_oversize, 2.0, "index 3 is 200%");
    assert_eq!(q.sdf_scale, 0.25, "index 2 is 25%");
}

/// The whole point of the bead's wiring requirement: a `project.godot` flag
/// changes the Play frame. Snapping read from settings must move the raster the
/// same way setting the flag by hand does.
#[test]
fn a_project_setting_changes_the_rendered_frame() {
    use gdcore::project_settings::{ProjectSettings, SettingsValue};

    let mut ps = ProjectSettings::default();
    ps.set(
        "rendering/2d/snap/snap_2d_transforms_to_pixel",
        SettingsValue::Bool(true),
    );
    let from_settings = CanvasQuality::from_project_settings(&ps);

    // 10.5 is the offset where snapping is observable in this raster.
    let item = rect_item(Vector2::new(10.5, 10.0));
    assert_eq!(
        first_filled_column(&render(&[item.clone()], &from_settings), 12),
        Some(11),
        "project.godot's snap flag must reach the Play raster"
    );
    assert_eq!(
        first_filled_column(&render(&[item], &CanvasQuality::default()), 12),
        Some(10),
        "and the same scene without the setting must differ"
    );
}
