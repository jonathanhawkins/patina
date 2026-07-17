//! pat-nzgdu.157 — `RichTextEffect`/`CharFXTransform` registration and per-glyph
//! BBCode effects on `RichTextLabel`.
//!
//! Drives the real control render path (`render_control_tree`) against a real
//! `RichTextLabel` node and asserts on the emitted `DrawCommand`s — the same
//! commands a viewport would rasterise. A `[wave]` run must be drawn glyph by
//! glyph with per-glyph vertical offsets that change as the label's elapsed
//! time advances; untagged text must still be drawn as one string.
//!
//! Scope note (see the bead report): the *custom* half of this bead — a
//! script-defined `RichTextEffect` subclass with `bbcode = "ghost"` receiving a
//! `CharFXTransform` per character — is NOT covered here, because it is not
//! implementable yet. A scripted effect is a Resource carrying a script, and
//! resources in this engine (`ResourceRef { path, class_name, properties }`)
//! have no script slot, so nothing can invoke `_process_custom_fx`. The ClassDB
//! surface for both classes is registered and asserted below; the dispatch
//! needs a script-backed Resource layer first.

use gdcore::math::{Color, Vector2};
use gdscene::control::{self, AnchorPreset};
use gdscene::control_render::render_control_tree;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::theme::Theme;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

/// Builds a RichTextLabel showing `markup` at time `elapsed`, and returns the
/// draw commands the renderer emits for it.
fn draw(markup: &str, elapsed: f64) -> Vec<DrawCommand> {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("R", "RichTextLabel");
    n.set_property("bbcode_text", Variant::String(markup.to_owned()));
    n.set_property("fx_elapsed_time", Variant::Float(elapsed));
    let id: NodeId = tree.add_child(root, n).unwrap();
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
            DrawCommand::DrawString { text, position, .. } => {
                Some((text.clone(), *position))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn richtext_effect_classes_are_registered() {
    gdobject::register_2d_classes();

    // The bead's first acceptance clause.
    assert!(
        gdobject::class_exists("RichTextEffect"),
        "ClassDB must know RichTextEffect"
    );
    assert!(
        gdobject::is_parent_class("RichTextEffect", "Resource"),
        "RichTextEffect is a Resource in Godot"
    );
    assert!(
        gdobject::is_parent_class("RichTextEffect", "Object"),
        "…and therefore reaches Object — an unregistered base would sever the chain"
    );
    assert!(
        gdobject::class_has_method("RichTextEffect", "_process_custom_fx"),
        "the virtual a scripted effect overrides"
    );

    // Its payload type.
    assert!(gdobject::class_exists("CharFXTransform"));
    assert!(
        gdobject::is_parent_class("CharFXTransform", "RefCounted"),
        "CharFXTransform is a RefCounted in Godot"
    );
    assert!(
        gdobject::is_parent_class("CharFXTransform", "Object"),
        "RefCounted must itself be registered, or the chain breaks"
    );
    let props = gdobject::get_property_list("CharFXTransform");
    for want in ["offset", "color", "elapsed_time", "relative_index", "range"] {
        assert!(
            props.iter().any(|p| p.name == want),
            "CharFXTransform must expose `{want}`, got {:?}",
            props.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
}

#[test]
fn plain_text_is_still_drawn_as_one_run() {
    // The fast path must survive: no effect => no per-glyph explosion.
    let g = glyphs(&draw("hello", 0.0));
    assert_eq!(g.len(), 1, "untagged text is a single draw call, got {g:?}");
    assert_eq!(g[0].0, "hello");
}

#[test]
fn wave_draws_each_glyph_separately() {
    let g = glyphs(&draw("[wave]abc[/wave]", 0.0));
    assert_eq!(
        g.len(),
        3,
        "an effected run is drawn glyph-by-glyph so each can move, got {g:?}"
    );
    let text: String = g.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(text, "abc", "and the text itself is unchanged");
}

/// The bead's second acceptance clause: `[wave]` offsets glyph positions, and
/// those offsets change from frame to frame.
#[test]
fn richtext_effect_wave_offsets_glyphs_over_time() {
    let at0 = glyphs(&draw("[wave]abcd[/wave]", 0.0));
    let at1 = glyphs(&draw("[wave]abcd[/wave]", 0.1));
    assert_eq!(at0.len(), 4);
    assert_eq!(at1.len(), 4);

    // Per frame: at least one glyph sits somewhere new.
    assert!(
        at0.iter().zip(&at1).any(|((_, a), (_, b))| (a.y - b.y).abs() > 1e-3),
        "advancing fx_elapsed_time must move the wave: {at0:?} vs {at1:?}"
    );

    // Per glyph: neighbours are out of phase, i.e. it is a travelling wave and
    // not the whole run bobbing as one block.
    let ys: Vec<f32> = at0.iter().map(|(_, p)| p.y).collect();
    assert!(
        ys.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3),
        "adjacent glyphs must differ vertically at a fixed instant: {ys:?}"
    );

    // Horizontal advance is unchanged by the effect — a wave lifts glyphs, it
    // does not re-flow the line.
    let xs0: Vec<f32> = at0.iter().map(|(_, p)| p.x).collect();
    let xs1: Vec<f32> = at1.iter().map(|(_, p)| p.x).collect();
    assert_eq!(xs0, xs1, "x advance must not depend on time");
    assert!(
        xs0.windows(2).all(|w| w[1] > w[0]),
        "glyphs still advance left-to-right: {xs0:?}"
    );
}

#[test]
fn wave_tag_is_stripped_and_untagged_text_is_unaffected() {
    // Text outside the tag keeps the one-run fast path and sits at a fixed y.
    let a = glyphs(&draw("hi [wave]xy[/wave] bye", 0.0));
    let b = glyphs(&draw("hi [wave]xy[/wave] bye", 0.37));
    let joined: String = a.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(joined, "hi xy bye", "tags must not survive into the output");

    // First and last commands are the untagged runs; they must not move.
    assert_eq!(a.first().unwrap().1, b.first().unwrap().1, "leading run is static");
    assert_eq!(a.last().unwrap().1, b.last().unwrap().1, "trailing run is static");
}

#[test]
fn effects_phase_from_the_tag_not_the_label_start() {
    // `[wave]` opening mid-string must phase from its own first glyph, so the
    // run looks the same wherever the tag sits. (relative_index, not absolute.)
    let early = glyphs(&draw("[wave]ab[/wave]", 0.0));
    let late = glyphs(&draw("zzzz[wave]ab[/wave]", 0.0));
    let early_dy = early[1].1.y - early[0].1.y;
    // `late[0]` is the untagged "zzzz" run; the waved glyphs follow it.
    let late_dy = late[2].1.y - late[1].1.y;
    assert!(
        (early_dy - late_dy).abs() < 1e-4,
        "a run's shape must not depend on where the tag starts: {early_dy} vs {late_dy}"
    );
}

/// A nested tag splits an effected run into several spans, but it is still ONE
/// travelling wave: the phase must keep advancing across the split rather than
/// restarting. Regression — computing each sub-run's range from its own bounds
/// made every piece restart at relative_index 0, so the glyphs bobbed in phase
/// together instead of travelling.
#[test]
fn effect_phase_continues_across_a_nested_tag() {
    // `[wave]abcd[/wave]` and the same text with a colour tag splitting it must
    // put every glyph at the same height — the colour must not touch the wave.
    let plain = glyphs(&draw("[wave]abcd[/wave]", 0.0));
    let split = glyphs(&draw("[wave]ab[color=#ff0000]cd[/color][/wave]", 0.0));
    assert_eq!(plain.len(), 4);
    assert_eq!(split.len(), 4, "still one glyph per character");

    for (i, ((_, a), (_, b))) in plain.iter().zip(&split).enumerate() {
        assert!(
            (a.y - b.y).abs() < 1e-4,
            "glyph {i} must sit at the same height with or without the nested \
             tag (phase restarted?): {} vs {}",
            a.y,
            b.y
        );
    }

    // And the split really did recolour the tail, so the test isn't vacuous.
    let colors: Vec<Color> = draw("[wave]ab[color=#ff0000]cd[/color][/wave]", 0.0)
        .iter()
        .filter_map(|c| match c {
            DrawCommand::DrawString { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_ne!(colors[0], colors[3], "the nested colour tag must still apply");
}

#[test]
fn unknown_and_custom_effect_tags_are_stripped_not_rendered() {
    // `ghost` is a *script-defined* effect. It cannot be dispatched yet (no
    // script-backed Resources), so it must degrade to plain text rather than
    // leaking the tag into the output or panicking.
    let g = glyphs(&draw("[ghost]boo[/ghost]", 0.0));
    let joined: String = g.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(joined, "boo", "an undispatchable effect renders its text plainly");
    assert_eq!(g.len(), 1, "and takes the un-effected fast path");
}

#[test]
fn shake_and_rainbow_tags_are_recognised_end_to_end() {
    // Two more built-ins reach the renderer: shake moves glyphs, rainbow tints
    // them without moving them.
    let shaken = glyphs(&draw("[shake]abc[/shake]", 0.5));
    assert_eq!(shaken.len(), 3, "shake is per-glyph");

    let cmds = draw("[rainbow]abc[/rainbow]", 0.0);
    let colors: Vec<Color> = cmds
        .iter()
        .filter_map(|c| match c {
            DrawCommand::DrawString { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(colors.len(), 3);
    assert!(
        colors.windows(2).any(|w| w[0] != w[1]),
        "rainbow must tint glyphs differently along the run: {colors:?}"
    );
}
