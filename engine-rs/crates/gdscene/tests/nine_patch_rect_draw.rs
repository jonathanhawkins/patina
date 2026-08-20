//! pat-0q2kx.179 — `NinePatchRect` must actually emit a nine-patch draw.
//!
//! The nine-patch rasterizer has existed in `gdrender2d` since before this bead
//! with **no producer**: `control_render::draw_control` had arms for
//! ColorRect/Button/Label/TextureRect/… and then `_ => {}`, so a `NinePatchRect`
//! in a scene fell through and drew **nothing at all**. Textbook wired-but-inert
//! — the expensive half existed and nothing reached it.
//!
//! These assert the real `DrawCommand::DrawNinePatch` off the viewport's canvas,
//! not the flattened `ControlDrawItem`: that flattening collapses a nine-patch to
//! a plain `Texture` item and discards the margins, so asserting through it could
//! not tell a nine-patch from an ordinary texture rect.

use gdcore::math::{Color, Rect2, Vector2};
use gdscene::control::AnchorPreset;
use gdscene::control_render::render_control_tree;
use gdscene::node::Node;
use gdscene::theme::Theme;
use gdscene::SceneTree;
use gdserver2d::canvas::{DrawCommand, NinePatchAxis};
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// Builds `Root -> NinePatchRect` with the given extra properties and returns
/// every draw command the control tree produced.
fn draw_commands(props: &[(&str, Variant)]) -> Vec<DrawCommand> {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let np = tree
        .add_child(root, Node::new("Frame", "NinePatchRect"))
        .expect("node added");
    if let Some(node) = tree.get_node_mut(np) {
        node.set_property("texture", Variant::String("res://frame.png".into()));
        for (k, v) in props {
            node.set_property(k, v.clone());
        }
    }
    // ⚠️ Anchors + offsets, NOT `size` (pat-s3rrv.167).
    //
    // The render walk runs `resolve_control_layout`, which RECOMPUTES `size`
    // from the anchor/offset pair — so a fixture that writes `size` directly is
    // silently reset to 0x0 before anything is drawn, and every rect assertion
    // then compares against a dead rect.
    //
    // This lay dormant while `NinePatchRect` was missing from `CONTROL_CLASSES`:
    // the node was skipped entirely, so the layout pass never touched it. Fixing
    // the membership made the node a real Control and the latent fixture bug
    // surfaced immediately. Same idiom as the RichTextLabel suites.
    gdscene::control::apply_anchor_preset(&mut tree, np, AnchorPreset::TopLeft);
    gdscene::control::set_offset_right(&mut tree, np, 96.0);
    gdscene::control::set_offset_bottom(&mut tree, np, 96.0);

    let theme = Theme::default();
    let mut viewport = Viewport::new(128, 128, Color::new(0.0, 0.0, 0.0, 0.0));
    render_control_tree(
        &mut tree,
        root,
        &theme,
        &mut viewport,
        Vector2::new(128.0, 128.0),
    );
    viewport
        .get_sorted_items()
        .iter()
        .flat_map(|item| item.commands.iter().cloned())
        .collect()
}

fn nine_patch(cmds: &[DrawCommand]) -> Option<&DrawCommand> {
    cmds.iter()
        .find(|c| matches!(c, DrawCommand::DrawNinePatch { .. }))
}

/// **The wiring test.** Before this bead the class produced no command at all.
#[test]
fn nine_patch_rect_emits_a_nine_patch_command_with_its_margins() {
    let cmds = draw_commands(&[
        ("patch_margin_left", Variant::Int(8)),
        ("patch_margin_top", Variant::Int(8)),
        ("patch_margin_right", Variant::Int(8)),
        ("patch_margin_bottom", Variant::Int(8)),
    ]);

    let Some(DrawCommand::DrawNinePatch {
        texture_path,
        rect,
        margin_left,
        margin_top,
        margin_right,
        margin_bottom,
        draw_center,
        ..
    }) = nine_patch(&cmds)
    else {
        panic!("NinePatchRect must emit a DrawNinePatch; got {cmds:?}");
    };

    assert_eq!(texture_path, "res://frame.png");
    assert_eq!(rect.size, Vector2::new(96.0, 96.0));
    assert_eq!(
        (*margin_left, *margin_top, *margin_right, *margin_bottom),
        (8.0, 8.0, 8.0, 8.0),
        "the patch_margin_* properties must reach the command"
    );
    assert!(
        *draw_center,
        "draw_center defaults to true in Godot; an absent property must not blank the centre"
    );
}

/// `draw_center = false` must reach the command — the hollow-frame case.
#[test]
fn draw_center_false_is_carried_through() {
    let cmds = draw_commands(&[
        ("patch_margin_left", Variant::Int(4)),
        ("draw_center", Variant::Bool(false)),
    ]);
    let Some(DrawCommand::DrawNinePatch { draw_center, .. }) = nine_patch(&cmds) else {
        panic!("expected a DrawNinePatch; got {cmds:?}");
    };
    assert!(!*draw_center);
}

/// The per-axis stretch modes map through Godot's `AXIS_STRETCH_MODE_*`
/// constants (STRETCH=0, TILE=1, TILE_FIT=2), reusing the same table
/// `StyleBoxTexture` uses so the two cannot drift.
#[test]
fn axis_stretch_modes_map_from_godot_constants() {
    let cmds = draw_commands(&[
        ("axis_stretch_horizontal", Variant::Int(1)),
        ("axis_stretch_vertical", Variant::Int(2)),
    ]);
    let Some(DrawCommand::DrawNinePatch { axis_h, axis_v, .. }) = nine_patch(&cmds) else {
        panic!("expected a DrawNinePatch; got {cmds:?}");
    };
    assert_eq!(*axis_h, NinePatchAxis::Tile, "1 = AXIS_STRETCH_MODE_TILE");
    assert_eq!(
        *axis_v,
        NinePatchAxis::TileFit,
        "2 = AXIS_STRETCH_MODE_TILE_FIT"
    );
}

/// Defaults: no axis properties means Stretch on both axes, and no margins means
/// zero — a plain stretched texture rather than a panic or a skipped command.
#[test]
fn absent_properties_fall_back_to_godot_defaults() {
    let cmds = draw_commands(&[]);
    let Some(DrawCommand::DrawNinePatch {
        axis_h,
        axis_v,
        margin_left,
        ..
    }) = nine_patch(&cmds)
    else {
        panic!("a NinePatchRect with only a texture must still draw; got {cmds:?}");
    };
    assert_eq!(*axis_h, NinePatchAxis::Stretch);
    assert_eq!(*axis_v, NinePatchAxis::Stretch);
    assert_eq!(*margin_left, 0.0);
}

/// pat-s3rrv.167 clause 4 — `region_rect` reaches the draw command.
///
/// Godot's `NinePatchRect.region_rect` confines the nine patches to a sub-rect
/// of the SOURCE texture: with a region set, the corner patches are cut from the
/// region's corners rather than the texture's. Both renderer backends already
/// consume the `region` field; nothing asserted that the node's authored value
/// ever arrives there.
///
/// ⚠️ Asserted at the DRAW COMMAND rather than on pixels, deliberately. The
/// sampling itself belongs to `gdrender2d` and is its to test; what this bead
/// wires is producer → command, and that is the link that was unasserted. A
/// pixel test here would pass or fail for reasons owned by another crate.
#[test]
fn region_rect_is_carried_into_the_nine_patch_command() {
    let cmds = draw_commands(&[(
        "region_rect",
        Variant::Rect2(Rect2::new(
            Vector2::new(16.0, 16.0),
            Vector2::new(32.0, 32.0),
        )),
    )]);
    let Some(DrawCommand::DrawNinePatch { region, .. }) = nine_patch(&cmds) else {
        panic!("expected a DrawNinePatch, got {cmds:?}");
    };
    assert_eq!(
        (region.position.x, region.position.y),
        (16.0, 16.0),
        "the authored region_rect origin must reach the command, got {region:?}"
    );
    assert_eq!(
        (region.size.x, region.size.y),
        (32.0, 32.0),
        "…and its size, got {region:?}"
    );
}

/// The default: an unauthored `region_rect` stays zero-size, which every
/// consumer reads as "use the whole texture".
///
/// ⚠️ This is the byte-for-byte clause. A zero `Rect2` is Godot's own default,
/// and the renderers branch on it — so a producer that "helpfully" substituted
/// the texture bounds here would change every existing nine-patch draw while
/// looking more correct.
#[test]
fn an_unauthored_region_rect_stays_zero_sized() {
    let cmds = draw_commands(&[]);
    let Some(DrawCommand::DrawNinePatch { region, .. }) = nine_patch(&cmds) else {
        panic!("expected a DrawNinePatch, got {cmds:?}");
    };
    assert_eq!(
        (region.size.x, region.size.y),
        (0.0, 0.0),
        "an unauthored region_rect must stay zero-size — a non-zero default \
         would silently re-crop every existing nine-patch, got {region:?}"
    );
}
/// A `NinePatchRect` with no texture must emit nothing rather than a command
/// pointing at an empty path.
#[test]
fn no_texture_emits_no_command() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let np = tree
        .add_child(root, Node::new("Frame", "NinePatchRect"))
        .unwrap();
    if let Some(node) = tree.get_node_mut(np) {
        node.set_property("size", Variant::Vector2(Vector2::new(96.0, 96.0)));
    }
    let theme = Theme::default();
    let mut viewport = Viewport::new(128, 128, Color::new(0.0, 0.0, 0.0, 0.0));
    render_control_tree(
        &mut tree,
        root,
        &theme,
        &mut viewport,
        Vector2::new(128.0, 128.0),
    );
    let cmds: Vec<DrawCommand> = viewport
        .get_sorted_items()
        .iter()
        .flat_map(|i| i.commands.iter().cloned())
        .collect();
    assert!(
        nine_patch(&cmds).is_none(),
        "an untextured NinePatchRect must not emit a command, got {cmds:?}"
    );
}
