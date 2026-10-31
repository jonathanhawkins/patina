//! pat-s3rrv.92 acceptance: `Sprite3D`, `Label3D` and `AnimatedSprite3D` reach
//! the editor 3D render path as textured quads instead of three identical grey
//! marker cubes.
//!
//! Godot ground truth: `upstream/godot/scene/3d/sprite_3d.cpp` (SpriteBase3D /
//! Sprite3D / AnimatedSprite3D) and `scene/3d/label_3d.cpp`.
//!
//! ⚠️ **All three ALREADY appeared in the viewport before this bead** — that is
//! what made the gap easy to miss. They descend from `Node3D`, so
//! `collect_scene_content` emitted an item for each; with no mesh resource and
//! no collision shape they fell through to the default marker primitive. The
//! bug was never "nothing renders", it was "everything renders as the same
//! cube", so every assertion here has to pin the pixels a QUAD produces, not
//! merely that some pixels changed.

use gdcore::math::Color;
use gdeditor::sprite3d_render::sprite3d_geometry;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{render_editor_3d_frame, render_game_3d_frame};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::MaterialTextureCache;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A 1x1 opaque red PNG that already ships as a fixture.
///
/// ⚠️ Its being 1x1 is deliberate, not a shortcut: `pixel_size` is authored per
/// node, so a 1x1 texture at `pixel_size = 2.0` is a 2x2 WORLD-UNIT quad. That
/// keeps the test independent of any fixture's dimensions while still covering
/// a large, unambiguous block of the frame. A bare filesystem path is passed
/// straight through by `MaterialTextureCache::load`, which documents exactly
/// this use ("so tests and tools can load a texture without a project").
const RED_TEX: &str = "../gdresource/fixtures/red1x1.png";

fn scene(sprite_visible: bool, label_text: &str) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Sprite" type="Sprite3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, -2, 0, 0)
texture = "{RED_TEX}"
pixel_size = 2.0
visible = {sprite_visible}

[node name="Label" type="Label3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 0, 0)
text = "{label_text}"
pixel_size = 0.05
"#
    )
}

fn load(tscn: &str) -> SceneTree {
    // ⚠️ Register FIRST. `collect_scene_content` filters on
    // `node.is_class("Node3D")`, which walks ClassDB — with an empty registry
    // every node is filtered out and the frame comes back empty, which reads
    // like the render path dropped the nodes rather than like the test never
    // populated the class table.
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn render(tree: &SceneTree) -> Vec<Color> {
    let mut vp = Viewport3D::default();
    vp.width = 96;
    vp.height = 96;
    render_editor_3d_frame(&vp, tree).pixels
}

/// Hue test rather than an exact colour: the sprite is unlit so it keeps its
/// texture's red, but the backdrop is a gradient and an absolute comparison
/// would be pinned to whatever shade the editor paints behind it.
///
/// ⚠️ **THIS MATCHES THE EDITOR'S X-AXIS OVERLAY TOO** — `X_AXIS_COLOR_3D` is
/// `(0.86, 0.32, 0.34)`, drawn over every frame at the origin. So a COUNT of
/// reddish pixels is never sprite-specific, and every assertion below has to be
/// a DIFFERENCE between two frames rather than an absolute number. An earlier
/// version asserted `count_red(shown) > 50` and passed on the axis alone,
/// without the sprite contributing anything.
fn reddish(c: &Color) -> bool {
    c.r > c.g + 0.2 && c.r > c.b + 0.2
}

fn count_red(pixels: &[Color]) -> usize {
    pixels.iter().filter(|c| reddish(c)).count()
}

fn differing(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| x.r != y.r || x.g != y.g || x.b != y.b)
        .count()
}

/// **The acceptance test.**
#[test]
fn sprite3d_label3d_animatedsprite3d_render_from_scene() {
    let shown = render(&load(&scene(true, "HI")));
    let hidden = render(&load(&scene(false, "HI")));

    // --- The sprite draws its TEXTURE, not a marker cube, AND hiding it takes
    //     those pixels away. Both claims are one subtraction: the editor's
    //     reddish X-axis overlay appears in both frames and cancels, so what is
    //     left is the sprite's own contribution.
    //
    //     A grey marker cube cannot produce red pixels at any light level, so a
    //     surplus of RED specifically is what separates "drew the quad" from
    //     "drew something". The quad is ~2x2 world units at distance 6 in a
    //     96x96 frame, so it covers several hundred pixels; 50 is a floor well
    //     clear of the axis line's own width.
    let red_shown = count_red(&shown);
    let red_hidden = count_red(&hidden);
    assert!(
        red_shown >= red_hidden + 50,
        "a visible red Sprite3D quad must add reddish pixels that hiding it \
         removes; got {red_shown} shown vs {red_hidden} hidden"
    );
    assert!(
        differing(&shown, &hidden) >= red_shown - red_hidden,
        "hiding the sprite must change at least the pixels it was covering"
    );

    // --- The label draws READABLE TEXT: different text must produce a
    //     different frame. A marker cube — or a quad that ignored the string —
    //     renders identically whatever the text says, so this is the assertion
    //     that a cube cannot pass.
    let other_text = render(&load(&scene(true, "WW")));
    assert!(
        differing(&shown, &other_text) > 0,
        "changing Label3D.text must change the rendered pixels; identical \
         frames mean the text never reached the quad"
    );

    // --- ...and an empty label falls back to the marker primitive, so a
    //     labelled scene differs from an unlabelled one too.
    //
    // ⚠️ The "WW" comparison above is the stronger of the two and the reason
    // both are here: "HI" and "WW" are the same character count, so they build
    // an IDENTICALLY SIZED quad and the only thing that can differ is the
    // glyph pixels. This empty-text case would also pass for a quad that
    // ignored the string entirely, since the fallback changes the shape.
    let no_text = render(&load(&scene(true, "")));
    assert!(
        differing(&shown, &no_text) > 0,
        "a Label3D with text must differ from one without"
    );
}

/// `AnimatedSprite3D` advances through its `SpriteFrames` and republishes the
/// current frame's texture.
///
/// ⚠️ Asserted through the NODE'S OWN `texture` property, because that is the
/// contract the viewport depends on: `process_animated_sprites` publishes each
/// frame there so the 3D path resolves a sprite's pixels exactly as the 2D
/// canvas does, rather than through a second animation route that could
/// disagree about which frame is showing.
#[test]
fn an_animatedsprite3d_advances_its_frame_and_republishes_its_texture() {
    let mut tree = load(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Anim" type="AnimatedSprite3D" parent="."]
"#,
    );
    let anim = tree.find_child(tree.root_id(), "Anim", true).expect("node");
    {
        let node = tree.get_node_mut(anim).expect("node");
        node.set_property(
            "__sf_anims__",
            Variant::PackedStringArray(vec!["default".into()]),
        );
        // ⚠️ THREE frames, not two. See the delta below: with two frames at
        // 10 fps, any whole multiple of 0.2s lands exactly on a loop wrap and
        // `frame` returns to 0 — which is indistinguishable from a driver that
        // never advanced at all.
        node.set_property(
            "__sf_default_frames",
            Variant::PackedStringArray(vec!["a.png".into(), "b.png".into(), "c.png".into()]),
        );
        node.set_property("__sf_default_speed", Variant::Float(10.0));
        node.set_property("__sf_default_loop", Variant::Bool(true));
        node.set_property("animation", Variant::String("default".into()));
        node.set_property("playing", Variant::Bool(true));
    }

    let frame_of = |t: &SceneTree| match t.get_node(anim).map(|n| n.get_property("frame")) {
        Some(Variant::Int(i)) => i,
        _ => -1,
    };
    assert_eq!(frame_of(&tree), 0, "starts on frame 0");

    // ⚠️ **A STEPPED-TIME TEST MUST NOT LAND ON A BOUNDARY.** At 10 fps each
    // frame lasts 0.1s. 0.15s is one and a half frames, so it sits strictly
    // between steps: it cannot be mistaken for "no advance" whether the driver
    // floors or rounds, and with three frames it cannot wrap to 0 either. My
    // first attempt used two frames and 0.2s — exactly two steps — and failed
    // with `left: 0, right: 0`, reporting a full loop as a dead driver.
    tree.process_animated_sprites(0.15);
    let advanced = frame_of(&tree);
    assert_ne!(
        advanced, 0,
        "an AnimatedSprite3D must advance; sitting on frame 0 is the symptom of \
         the driver still being gated to AnimatedSprite2D"
    );
    assert!(
        advanced == 1 || advanced == 2,
        "1.5 frames at 10 fps should land on frame 1, got {advanced}"
    );
    match tree.get_node(anim).map(|n| n.get_property("texture")) {
        Some(Variant::String(p)) => assert!(
            p.ends_with(".png"),
            "the current frame's texture must be published as `texture`, got {p:?}"
        ),
        other => panic!("texture should be published as a String, got {other:?}"),
    }
}

// ===========================================================================
// pat-mgixk.34: the published frame must reach PIXELS, not just the property
// ===========================================================================

/// A 1x1 opaque blue PNG, structurally identical to [`RED_TEX`] (1x1, 8-bit,
/// colour type 2, no interlace) and differing only in the pixel.
///
/// A SECOND colour is what makes this test about *which* frame is showing. With
/// one texture the only observable would be "some pixels appeared", which a
/// marker cube also satisfies.
const BLUE_TEX: &str = "../gdresource/fixtures/blue1x1.png";

/// Renders the way the **game** sees it: `render_game_3d_frame` takes the
/// scene's own `Camera3D` and turns the editor furniture off.
///
/// ⚠️ Deliberately NOT the editor path used above. The editor draws the origin
/// axes over every frame, and `X_AXIS_COLOR_3D` is reddish — which is why the
/// assertions in the earlier test had to be frame-to-frame differences rather
/// than absolute counts. The game path has no axes, so a colour count here is
/// genuinely the sprite's.
fn render_game(tree: &SceneTree) -> Vec<Color> {
    render_game_3d_frame(tree, 96, 96)
        .expect("the scene has a current Camera3D, so a game frame must render")
        .pixels
}

fn bluish(c: &Color) -> bool {
    c.b > c.r + 0.2 && c.b > c.g + 0.2
}

fn count_blue(pixels: &[Color]) -> usize {
    pixels.iter().filter(|c| bluish(c)).count()
}

/// **The acceptance test (pat-mgixk.34 part 1).**
///
/// An `AnimatedSprite3D` draws its current `SpriteFrames` frame as a textured
/// quad, and advancing past the frame duration changes the pixels on screen.
///
/// Before this bead `sprite3d_geometry`'s class guard accepted only
/// "Sprite3D", so the node fell through to the default marker cube: the frame
/// advanced, the `texture` property was republished — and the renderer threw it
/// away. `an_animatedsprite3d_advances_its_frame_and_republishes_its_texture`
/// above already covered the publish half and passed throughout, which is
/// exactly why the gap survived. This test pins the half that was missing: the
/// pixels.
#[test]
fn animated_sprite3d_renders_frames() {
    let mut tree = load(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Anim" type="AnimatedSprite3D" parent="."]
pixel_size = 2.0
"#,
    );
    let anim = tree.find_child(tree.root_id(), "Anim", true).expect("node");
    {
        let node = tree.get_node_mut(anim).expect("node");
        // The `SpriteFrames` companion properties a `.tscn` resource lowers to,
        // same shape the sibling test above uses.
        node.set_property(
            "__sf_anims__",
            Variant::PackedStringArray(vec!["default".into()]),
        );
        node.set_property(
            "__sf_default_frames",
            Variant::PackedStringArray(vec![RED_TEX.into(), BLUE_TEX.into()]),
        );
        node.set_property("__sf_default_speed", Variant::Float(10.0));
        node.set_property("__sf_default_loop", Variant::Bool(true));
        // `autoplay` rather than `playing`: it is what the acceptance names, and
        // it exercises the autostart branch that selects the clip on the first
        // processed frame.
        node.set_property("autoplay", Variant::String("default".into()));
    }

    // 0.01s at 10 fps is a tenth of a frame: the clip starts and frame 0's
    // texture is published without advancing off it.
    tree.process_animated_sprites(0.01);

    // --- Diagnostic ladder -------------------------------------------------
    //
    // ⚠️ These run BEFORE the pixel assertions on purpose. The first version of
    // this test failed with `red=0, blue=0` — NOTHING drew — and that single
    // number cannot distinguish "the tick never published a texture" from "the
    // path did not resolve" from "the decode failed" from "my guard still
    // rejects the class" from "the rasterizer is not drawing quads at all".
    // Each rung below names one link, so a failure points at the broken one
    // instead of leaving the next reader to re-derive the chain.
    let published = tree
        .get_node(anim)
        .map(|n| n.get_property("texture"))
        .unwrap_or(Variant::Nil);
    let published_path = match &published {
        Variant::String(p) => p.clone(),
        other => panic!(
            "LINK 1 (SceneTree tick): process_animated_sprites must publish \
             frame 0's path as `texture`; got {other:?}. Without this the \
             renderer has nothing to resolve."
        ),
    };
    assert_eq!(
        published_path, RED_TEX,
        "LINK 1: frame 0 of the SpriteFrames is the red texture"
    );
    assert!(
        std::path::Path::new(&published_path).is_file(),
        "LINK 2 (fixture path): `{published_path}` must be readable from the \
         test's working directory — `MaterialTextureCache::load` passes a bare \
         relative path straight to `PathBuf::from`, so this is CWD-sensitive"
    );
    let mut probe_cache = MaterialTextureCache::new();
    assert!(
        probe_cache.get_or_load(&published_path).is_some(),
        "LINK 3 (decode): the PNG at `{published_path}` must decode, or \
         sprite3d_geometry bails at its `cache.get_or_load(..)?`"
    );
    // LINK 4 is this bead's actual change: the class guard used to reject
    // AnimatedSprite3D here, so this returned None and the node fell through to
    // a marker cube. This is the rung that fails on HEAD.
    assert!(
        sprite3d_geometry(&tree, anim, &mut probe_cache).is_some(),
        "LINK 4 (class guard — THE BEAD): sprite3d_geometry must build a quad \
         for an AnimatedSprite3D. None here means the guard still accepts only \
         \"Sprite3D\"."
    );

    let frame0 = render_game(&tree);
    let (red0, blue0) = (count_red(&frame0), count_blue(&frame0));

    // LINK 5: the quad reaches the frame. With links 1-4 green, a failure here
    // is downstream of this bead — the shared render list or rasterizer — and
    // note that the sibling `sprite3d_label3d_animatedsprite3d_render_from_scene`
    // test, which renders a STATIC Sprite3D through the very same
    // `sprite3d_geometry`, is recorded red in
    // `docs/test-debt/2026-10-30-failing-tests-at-HEAD.txt`.
    assert!(
        red0 > 0,
        "LINK 5 (render list -> pixels): frame 0 is the red texture, so the quad \
         must put red pixels on screen (red={red0}, blue={blue0}). Links 1-4 \
         passed, so the texture resolved and the quad was built — this is the \
         shared Sprite3D draw path, not the AnimatedSprite3D guard."
    );

    // ⚠️ 0.15s = one and a half frames at 10 fps, chosen to sit STRICTLY
    // BETWEEN steps. The sibling test above records why: with two frames, a
    // whole multiple of 0.1s can land on a loop wrap and return to frame 0,
    // which is indistinguishable from a driver that never advanced.
    tree.process_animated_sprites(0.15);
    let frame1 = render_game(&tree);
    let (red1, blue1) = (count_red(&frame1), count_blue(&frame1));

    assert!(
        blue1 > 0,
        "after advancing past the frame duration the quad must show frame 1's \
         blue (red={red1}, blue={blue1})"
    );

    // ⚠️ DIFFERENCES between the two frames, not absolute zeros. The flat
    // backdrop (0.13, 0.14, 0.17) satisfies neither predicate, but the 3D path
    // can also draw a procedural sky, whose bluish band would make
    // `blue == 0` fail for a reason that has nothing to do with this bead. A
    // rise in blue and a fall in red is the sprite's own contribution whatever
    // is behind it.
    assert!(
        blue1 > blue0,
        "advancing to frame 1 must ADD blue pixels (frame0 blue={blue0}, \
         frame1 blue={blue1})"
    );
    assert!(
        red1 < red0,
        "frame 1 replaced frame 0, so red must fall (frame0 red={red0}, \
         frame1 red={red1})"
    );

    // The same quad covers the same area either way; only its colour changed.
    // This is what separates "the texture swapped" from "the geometry appeared
    // or vanished". Compared within a tolerance rather than exactly: edge
    // pixels blend with the backdrop, and the two colours need not round
    // across the predicate thresholds identically.
    let gained = blue1.saturating_sub(blue0) as i64;
    let lost = red0.saturating_sub(red1) as i64;
    assert!(
        (gained - lost).abs() * 4 <= lost.max(1),
        "the blue the quad gained should match the red it lost to within 25% — \
         a large mismatch means the geometry changed size, not just its texture \
         (gained={gained}, lost={lost})"
    );
}


/// A 2x1 sheet whose left texel is RED and right texel is BLUE, so "which cell is
/// on screen" is answerable from a single pixel's hue.
///
/// Structurally identical to the other fixtures in this crate — 8-bit, colortype 2
/// (RGB), non-interlaced — which is the form `import_image`'s fast path decodes.
const SHEET_TEX: &str = "../gdresource/fixtures/red_blue2x1.png";

/// A `Sprite3D` showing one cell of [`SHEET_TEX`].
///
/// `hframes = 2` with a 2-texel-wide texture makes each cell exactly one texel, and
/// `pixel_size = 2.0` scales that to a 2x2-unit quad — big enough to cover the
/// centre of frame from the camera 6 units back. A 1x1 cell at the default
/// `pixel_size` of 0.01 would be a one-centimetre quad and cover no pixels at all,
/// which is how the first version of the sibling test above failed.
fn sheet_scene(frame: i64) -> String {
    format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 6)
current = true

[node name="Sheet" type="Sprite3D" parent="."]
texture = "{SHEET_TEX}"
pixel_size = 2.0
hframes = 2
frame = {frame}
"#
    )
}

/// **pat-mgixk.34 part 2 (sprite-sheet slice): `hframes`/`frame` select one cell.**
///
/// Godot's `SpriteBase3D` divides the texture into an `hframes` x `vframes` grid and
/// draws `frame` alone (`scene/3d/sprite_3d.cpp`). Patina read none of those
/// properties: `grep '"hframes"' sprite3d_render.rs` was empty, so every Sprite3D
/// stretched the WHOLE sheet across its quad — a spritesheet character showed all
/// of its frames at once.
///
/// ⚠️ Asserted as a DIFFERENCE between the two frames, not as absolute zeros. The 3D
/// path can draw a procedural sky whose bluish band would make `blue == 0` fail for
/// a reason unrelated to this bead; the sibling test above records the same trap.
/// A rise in one hue and a fall in the other is the cell's own contribution whatever
/// is behind it.
#[test]
fn sprite3d_hframes_frame_selects_one_cell() {
    let frame0 = render_game(&load(&sheet_scene(0)));
    let frame1 = render_game(&load(&sheet_scene(1)));

    let (red0, blue0) = (count_red(&frame0), count_blue(&frame0));
    let (red1, blue1) = (count_red(&frame1), count_blue(&frame1));

    // frame 0 is the sheet's LEFT texel, which is red.
    assert!(
        red0 > 0,
        "frame 0 selects the sheet's red cell, so red pixels must reach the frame \
         (red={red0}, blue={blue0}). Zero means `hframes`/`frame` are still unread \
         and the quad is sampling the whole sheet"
    );
    // frame 1 is the RIGHT texel, which is blue.
    assert!(
        blue1 > 0,
        "frame 1 selects the sheet's blue cell (red={red1}, blue={blue1})"
    );

    // …and each frame shows ONLY its own cell. This is the half that catches a quad
    // still sampling the full 0..1 UV range: that would put both hues on screen in
    // both frames, so neither difference below would hold.
    assert!(
        red0 > red1,
        "frame 1 must drop the red cell: red went {red0} -> {red1}"
    );
    assert!(
        blue1 > blue0,
        "frame 0 must not show the blue cell: blue went {blue0} -> {blue1}"
    );
}
