//! pat-mgixk.74 acceptance — the web runtime renders the running SceneTree into
//! the canvas under the project's stretch settings, and maps pointer events back
//! through the same transform.
//!
//! Godot ground truth: on the web every iteration draws the RenderingServer
//! frame for the root Window into the canvas, scaled per
//! `display/window/stretch/mode|aspect|scale`
//! (`platform/web/display_server_web.cpp`, `doc/classes/Window.xml`).
//!
//! ⚠️ WHAT WAS BROKEN WAS THE JOIN, NOT THE PARTS. `RootWindow` already computed
//! the stretch (`viewport.rs:91 stretch_transform`, `:101 visible_size`) and
//! already had the inverse for input (`:120 window_to_canvas`, whose own doc
//! calls itself "the inverse of the transform the renderer applies"). The
//! renderer never applied one: `WebEngine::render_2d_rgba` rasterized the scene
//! AT the size it was handed, so a 320x180 project on a 640x480 canvas drew its
//! nodes at project coordinates, 1:1, in the top-left corner — bigger canvas,
//! same-size content, no bars. And `CanvasMapping::to_project` stopped at canvas
//! pixels despite its name, so every click under stretch landed at the wrong
//! place.
//!
//! # The geometry this test pins
//!
//! 🔑 320x180 content in a 640x480 canvas with `aspect = keep`: the scale is
//! `min(640/320, 480/180) = min(2, 2.667) = 2`, so the content occupies
//! 640x360 and is centred vertically, leaving 60px black bars top and bottom.
//! Every expected number below is derived from that, and the `keep` aspect is
//! what makes the bars exist at all — under `ignore` the content would simply
//! distort to fill and there would be nothing to letterbox.

use gdcore::content_scale::{ContentScaleAspect, ContentScaleMode};
use gdcore::math::Vector2;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use patina_web::browser_input::CanvasMapping;
use patina_web::engine::WebEngine;

/// Project (design) resolution.
const BASE_W: f32 = 320.0;
const BASE_H: f32 = 180.0;
/// Canvas backing size — deliberately a DIFFERENT aspect (4:3 vs 16:9) so
/// `keep` has to letterbox.
const CANVAS_W: u32 = 640;
const CANVAS_H: u32 = 480;

/// Derived: uniform scale 2, content 640x360, 60px bars top and bottom.
const SCALE: f32 = 2.0;
const BAR_H: f32 = 60.0;

/// The rect in PROJECT coordinates.
const RECT_POS: (f32, f32) = (10.0, 10.0);
const RECT_SIZE: (f32, f32) = (20.0, 20.0);

/// `color = Color(0.2, 0.4, 0.8, 1)` in 8-bit: (51, 102, 204).
const RECT_RGB: [u8; 3] = [51, 102, 204];

const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Rect" type="ColorRect" parent="."]
position = Vector2(10, 10)
size = Vector2(20, 20)
color = Color(0.2, 0.4, 0.8, 1)
"#;

/// A `WebEngine` over the scene, with the project's stretch settings installed
/// on the root window.
///
/// ⚠️ Set on `root_window().config` rather than through `ProjectSettings`
/// deliberately: that config IS the runtime state the renderer reads
/// (`patina-runner/src/bootstrap.rs:462` installs exactly this from the settings
/// file), and `content_scale.rs`'s own tests already cover the settings->config
/// parse. Going through the file would test the loader twice and the join not at
/// all.
fn engine_with_scene() -> WebEngine {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    {
        let win = tree.root_window_mut();
        win.config.mode = ContentScaleMode::CanvasItems;
        win.config.aspect = ContentScaleAspect::Keep;
        win.config.base_size = Vector2::new(BASE_W, BASE_H);
    }
    WebEngine::new(tree)
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * CANVAS_W + x) as usize) * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[track_caller]
fn assert_rgb_near(got: [u8; 4], want: [u8; 3], what: &str) {
    let d = (got[0] as i32 - want[0] as i32).abs()
        + (got[1] as i32 - want[1] as i32).abs()
        + (got[2] as i32 - want[2] as i32).abs();
    assert!(
        d <= 9,
        "{what}: expected about {want:?}, got {:?}",
        &got[..3]
    );
}

#[test]
fn web_frame_renders_scene_pixels() {
    let mut engine = engine_with_scene();
    let buf = engine.render_frame_rgba(CANVAS_W, CANVAS_H, ".");
    assert_eq!(
        buf.len(),
        (CANVAS_W as usize) * (CANVAS_H as usize) * 4,
        "the frame must be a full RGBA buffer for the canvas backing size"
    );

    // -- (a) the rect is drawn SCALED and OFFSET, not at project coordinates --
    //
    // 🔑 The centre of where the rect should land after stretching. At project
    // (20, 20) — the rect's centre — scale 2 and a 60px bar give
    // (40, 60 + 40) = (40, 100).
    let cx = ((RECT_POS.0 + RECT_SIZE.0 / 2.0) * SCALE) as u32;
    let cy = ((RECT_POS.1 + RECT_SIZE.1 / 2.0) * SCALE + BAR_H) as u32;
    assert_rgb_near(
        px(&buf, cx, cy),
        RECT_RGB,
        &format!(
            "the ColorRect must be painted at its STRETCHED position ({cx}, \
             {cy}). Black here means nothing drew; finding it instead near \
             ({}, {}) — its raw project position — means the frame was \
             rasterized without the stretch transform, which is the old \
             behaviour",
            RECT_POS.0 as u32 + 10,
            RECT_POS.1 as u32 + 10
        ),
    );

    // ...and NOT at its unstretched position. ⚠️ This is the assertion that
    // fails on the old code: project (20, 20) is inside the top letterbox bar
    // once the stretch is applied, so it must be black.
    assert_eq!(
        px(&buf, 20, 20),
        [0, 0, 0, 255],
        "project coordinate (20, 20) lands inside the top letterbox bar after \
         stretching, so it must be BLACK. Finding the rect's colour here means \
         the scene was drawn at 1:1 in the corner — exactly what \
         render_2d_rgba did"
    );

    // -- (b) the letterbox bars are black -----------------------------------
    for (x, y, which) in [
        (CANVAS_W / 2, 5u32, "top"),
        (CANVAS_W / 2, CANVAS_H - 5, "bottom"),
    ] {
        assert_eq!(
            px(&buf, x, y),
            [0, 0, 0, 255],
            "the {which} letterbox bar must be opaque black; `keep` on a 16:9 \
             project in a 4:3 canvas leaves {BAR_H}px bars"
        );
    }
    // 🔑 The bar's thickness, measured rather than sampled. Scanning DOWN the
    // rect's own column, the first non-black pixel is the rect's top edge,
    // which sits at `project_y * scale + bar` = 10*2 + 60 = 80. That single
    // number pins the scale AND the margin together.
    //
    // ⚠️ An earlier version of this compared a row just inside the content area
    // against a row inside the bar and required them to DIFFER. That assertion
    // was unsound and cost a verify: away from the rect the content area is the
    // same black clear colour as a bar, so "inside the content" does not imply
    // "not black" in a sparse scene.
    let first_ink_y = (0..CANVAS_H).find(|y| px(&buf, cx, *y) != [0, 0, 0, 255]);
    assert_eq!(
        first_ink_y,
        Some((RECT_POS.1 * SCALE + BAR_H) as u32),
        "scanning down the rect's column, the first painted row must be its top \
         edge at project y={} scaled by {SCALE} plus the {BAR_H}px letterbox \
         bar. A value of {} would mean no bar was applied; None means the \
         column is empty",
        RECT_POS.1,
        (RECT_POS.1 * SCALE) as u32
    );

    // -- (c) a pointer at the rect's canvas position maps back INTO the rect --
    let project = engine.canvas_to_project(Vector2::new(cx as f32, cy as f32));
    assert!(
        project.x >= RECT_POS.0
            && project.x <= RECT_POS.0 + RECT_SIZE.0
            && project.y >= RECT_POS.1
            && project.y <= RECT_POS.1 + RECT_SIZE.1,
        "a pointer at canvas ({cx}, {cy}) must map back inside the rect's \
         project-space rect {RECT_POS:?}+{RECT_SIZE:?}; got {project:?}. \
         Unstretched, canvas pixels pass through unchanged and this lands far \
         outside"
    );

    // ...and through `CanvasMapping`, which is what the browser listeners use.
    // ⚠️ Carries the SAME inverse the engine applies, taken from the same root
    // window, so the two can never disagree about where a click landed.
    //
    // 🔑 Built with `canvas_to_project_transform`, the exact accessor the wasm
    // listeners call. The wasm build once constructed `CanvasMapping` WITHOUT
    // this field (E0063 on wasm32 only), so pointer events skipped the stretch
    // entirely; asserting through the accessor pins the browser's real path.
    let inverse = engine.canvas_to_project_transform();
    let mapping = CanvasMapping {
        origin_css: Vector2::new(0.0, 0.0),
        device_pixel_ratio: 1.0,
        canvas_to_project: inverse,
    };
    let mapped = mapping.to_project(Vector2::new(cx as f32, cy as f32));
    assert!(
        (mapped.x - project.x).abs() < 0.01 && (mapped.y - project.y).abs() < 0.01,
        "CanvasMapping must agree with the engine's own inverse: engine said \
         {project:?}, mapping said {mapped:?}"
    );
}
