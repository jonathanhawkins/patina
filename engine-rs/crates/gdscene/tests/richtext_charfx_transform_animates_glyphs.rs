//! pat-fk17e.2 — **the acceptance test**: a script-defined `RichTextEffect`
//! installed in `RichTextLabel.custom_effects` is handed a `CharFXTransform`
//! once per covered glyph, per frame, and what it writes reaches the draw.
//!
//! Before this bead, `RichTextEffect` and `CharFXTransform` were registered in
//! ClassDB and the six built-in `[wave]`/`[shake]`/… effects worked, but a
//! `[mytag]` naming a script-defined effect was stripped and its text drew
//! plainly — `richtext_effect_custom_fx_test` still pins that behaviour for an
//! effect that is NOT installed.
//!
//! How the pieces meet, since it is not obvious from any single file:
//!
//! 1. `control_render::flush_custom_char_fx` is a pre-pass, run where the
//!    scripted-StyleBox pre-pass already runs, because calling GDScript needs
//!    `&mut SceneTree` and the draw path holds the tree immutably by design.
//! 2. It hands the script a `CharFXTransform` as a phantom node, so
//!    `char_fx.offset = …` is serviced by the ordinary node-property path and
//!    the mutation lands somewhere the pre-pass can read it back.
//! 3. It caches the per-glyph results on the label as `__charfx_*__` packed
//!    arrays, and the draw path reads them instead of calling the (inert)
//!    `BuiltinEffect::Custom` arm of `apply`.

use gdcore::math::{Color, Vector2};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdscene::GDScriptNodeInstance;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// Slides each glyph right in proportion to `elapsed_time`.
///
/// ⚠️ Phased off `elapsed_time` and NOT off the glyph index, so the assertion
/// can be "the same glyph moved between two frames" rather than "glyphs differ
/// from each other" — the latter would also pass if the effect ran once and
/// never advanced.
const SLIDER: &str = "\
extends RichTextEffect

var bbcode = \"slide\"

func _process_custom_fx(char_fx):
    char_fx.offset = Vector2(char_fx.elapsed_time * 100.0, 0.0)
    return true
";

/// Returns `false`, which Godot treats as "do not draw this glyph".
const HIDER: &str = "\
extends RichTextEffect

var bbcode = \"hide\"

func _process_custom_fx(char_fx):
    return false
";

/// A RichTextLabel showing `markup`, with `effects` installed in
/// `custom_effects`, rendered at `elapsed`.
fn draw_with_effects(markup: &str, elapsed: f64, effects: &[(&str, &str)]) -> Vec<DrawCommand> {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    // Each effect is a script-backed object in the tree, which is what
    // pat-fk17e.1's foundation makes possible.
    let mut installed: Vec<Variant> = Vec::new();
    for (name, source) in effects {
        let node = tree
            .add_child(root, Node::new(*name, "RichTextEffect"))
            .unwrap();
        tree.attach_script(
            node,
            Box::new(GDScriptNodeInstance::from_source(source, node).expect("effect parses")),
        );
        installed.push(Variant::ObjectId(node.object_id()));
    }

    let mut label = Node::new("R", "RichTextLabel");
    label.set_property("bbcode_text", Variant::String(markup.to_owned()));
    label.set_property("fx_elapsed_time", Variant::Float(elapsed));
    label.set_property("custom_effects", Variant::Array(installed.into()));
    let id: NodeId = tree.add_child(root, label).unwrap();
    control::apply_anchor_preset(&mut tree, id, AnchorPreset::TopLeft);
    control::set_offset_right(&mut tree, id, 400.0);
    control::set_offset_bottom(&mut tree, id, 64.0);

    let mut vp = Viewport::new(512, 64, Color::BLACK);
    let theme = Theme::default_theme();
    render_control_tree(&mut tree, id, &theme, &mut vp, Vector2::new(512.0, 64.0));
    vp.get_sorted_items()
        .first()
        .map(|it| it.commands.clone())
        .unwrap_or_default()
}

/// The drawn glyphs as `(text, position)`, in emission order.
fn glyphs(cmds: &[DrawCommand]) -> Vec<(String, Vector2)> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCommand::DrawString { text, position, .. } => Some((text.clone(), *position)),
            _ => None,
        })
        .collect()
}

/// **The acceptance.** A custom effect setting `char_fx.offset` from
/// `elapsed_time` animates its glyphs across frames.
#[test]
fn richtext_charfx_transform_animates_glyphs() {
    let frame_1 = glyphs(&draw_with_effects(
        "[slide]abc[/slide]",
        0.1,
        &[("Slide", SLIDER)],
    ));
    let frame_10 = glyphs(&draw_with_effects(
        "[slide]abc[/slide]",
        1.0,
        &[("Slide", SLIDER)],
    ));

    // CONTROL: the tag was dispatched at all. Without this, a scene that never
    // resolved `[slide]` would draw "abc" as one run and the position
    // comparison below would be comparing nothing.
    assert_eq!(frame_1.len(), 3, "a dispatched effect draws per glyph");
    assert_eq!(frame_10.len(), 3);
    let text: String = frame_1.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(text, "abc", "the tag itself must not leak into the output");

    // THE ACCEPTANCE: the same glyph is in a different place between frames,
    // which is only true if the handler ran each frame and its write reached
    // the draw.
    assert_ne!(
        frame_1[0].1, frame_10[0].1,
        "glyph 0 must move between frame 1 and frame 10, got {:?} then {:?}",
        frame_1[0].1, frame_10[0].1
    );

    // And it moved the way the script asked: +X by elapsed * 100, so the later
    // frame is further right. Asserted as a direction rather than an exact
    // pixel so font metrics and the glyph advance cannot make this brittle.
    assert!(
        frame_10[0].1.x > frame_1[0].1.x,
        "the offset follows elapsed_time, got {} then {}",
        frame_1[0].1.x,
        frame_10[0].1.x
    );
}

/// **The second acceptance clause:** returning `false` hides the glyph.
///
/// ⚠️ This is the RETURN VALUE, not `char_fx.visible` — `HIDER` never touches
/// the property. Honouring only the property would pass a test written against
/// a handler that did both, which is the common shape, and silently fail for
/// one that returns false alone.
#[test]
fn returning_false_hides_the_glyph() {
    let shown = glyphs(&draw_with_effects(
        "[hide]abc[/hide]",
        0.0,
        &[("Hider", HIDER)],
    ));
    assert!(
        shown.is_empty(),
        "every covered glyph must be skipped, got {shown:?}"
    );
}

/// ⚠️ The control that makes the test above mean something: the SAME markup
/// with the effect NOT installed draws its text plainly. Without this, "no
/// glyphs" could equally mean the label failed to render at all.
#[test]
fn the_same_markup_without_the_effect_still_draws() {
    let plain = glyphs(&draw_with_effects("[hide]abc[/hide]", 0.0, &[]));
    let text: String = plain.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(text, "abc", "an uninstalled tag degrades to plain text");
}

/// Text outside the tag is untouched: the effect covers its own range only.
#[test]
fn only_the_tagged_range_is_affected() {
    let cmds = draw_with_effects("no[hide]yes[/hide]", 0.0, &[("Hider", HIDER)]);
    let text: String = glyphs(&cmds).iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        text, "no",
        "the untagged run survives, the tagged one is hidden"
    );
}

/// An effect whose `bbcode` does not match any tag in the markup changes
/// nothing — installing an effect is not enough, the tag has to name it.
#[test]
fn an_installed_effect_whose_tag_is_absent_does_nothing() {
    let cmds = draw_with_effects("[slide]abc[/slide]", 0.5, &[("Hider", HIDER)]);
    let text: String = glyphs(&cmds).iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        text, "abc",
        "`[slide]` must not reach an effect tagged `hide`"
    );
}
