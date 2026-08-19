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
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
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
