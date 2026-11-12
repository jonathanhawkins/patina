//! Acceptance test for pat-s3rrv.74: `[url=…]` runs are clickable and
//! `visible_characters` actually trims what is drawn.
//!
//! Godot ground truth: `scene/gui/rich_text_label.cpp` — `push_meta` attaches a
//! payload to a run and clicking it emits `meta_clicked(meta)`;
//! `visible_characters` / `visible_ratio` reveal the parsed text glyph by glyph
//! (tags never consume the budget, and `-1` means "all").
//!
//! ⚠️ Two different kinds of gap are closed here, and they fail differently:
//! - `meta_clicked` did not exist anywhere — `grep` returned **zero** non-test
//!   hits for it, `push_meta` and `append_text` alike.
//! - `visible_characters` and `visible_ratio` were **registered properties with
//!   no reader**: the inspector showed them and the value round-tripped through
//!   `.tscn`, and nothing consulted them. That is the harder failure to see,
//!   because every surface except the pixels says the feature is present.

use gdcore::math::{Color, Vector2};
use gdobject::signal::Connection;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, Theme};
use gdscript_interop::bindings::ScriptInstance;
use gdserver2d::canvas::DrawCommand;
use gdserver2d::viewport::Viewport;
use gdvariant::Variant;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A full-rect RichTextLabel carrying `bbcode`.
fn label(bbcode: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("R", "RichTextLabel");
    n.set_property("bbcode_enabled", Variant::Bool(true));
    n.set_property("bbcode_text", Variant::String(bbcode.to_owned()));
    let id = tree.add_child(root, n).unwrap();
    gdscene::control::set_position(&mut tree, id, Vector2::new(0.0, 0.0));
    gdscene::control::set_size(&mut tree, id, Vector2::new(300.0, 40.0));
    (tree, id)
}

/// Every string the label paints, in paint order.
fn drawn(tree: &mut SceneTree, id: NodeId) -> Vec<String> {
    let mut vp = Viewport::new(300, 40, Color::BLACK);
    let theme = Theme::default_theme();
    gdscene::control_render::render_control_tree(
        tree,
        id,
        &theme,
        &mut vp,
        Vector2::new(300.0, 40.0),
    );
    vp.get_sorted_items()
        .iter()
        .flat_map(|it| it.commands.clone())
        .filter_map(|c| match c {
            DrawCommand::DrawString { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn click(tree: &mut SceneTree, x: f32, y: f32) {
    for pressed in [true, false] {
        tree.push_input(&InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed,
            position: Vector2::new(x, y),
        });
    }
}

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

// ---------------------------------------------------------------------------
// [url] -> meta_clicked
// ---------------------------------------------------------------------------

/// Clicking a `[url=…]` run emits `meta_clicked` with its payload to a GDScript
/// handler.
#[test]
fn clicking_a_url_run_emits_meta_clicked_to_a_script() {
    let (mut tree, id) = label("go to [url=res://next.tscn]the link[/url] now");

    let src = "extends RichTextLabel\nvar got = \"\"\nfunc _on_meta(m):\n    self.got = m\n";
    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(src, id).unwrap()),
    );
    tree.connect_signal(
        id,
        "meta_clicked",
        Connection::new(id.object_id(), "_on_meta"),
    );

    // The link's own rect, measured the same way the draw pass lays it out —
    // asking the engine rather than hard-coding a pixel keeps the test honest
    // if the font metrics change.
    let regions = gdscene::control_render::richtext_meta_regions(
        &tree,
        id,
        gdscene::control_render::RICHTEXT_DEFAULT_FONT_SIZE,
        gdcore::math::Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(300.0, 40.0)),
    );
    assert_eq!(regions.len(), 1, "one [url] run, got {regions:?}");
    let (rect, meta) = regions[0].clone();
    assert_eq!(
        meta, "res://next.tscn",
        "the payload keeps its original case"
    );

    click(
        &mut tree,
        rect.position.x + rect.size.x * 0.5,
        rect.position.y + 5.0,
    );

    assert_eq!(
        script_var(&tree, id, "got"),
        Variant::String("res://next.tscn".into()),
        "clicking the link must emit meta_clicked with the url"
    );
}

/// ⚠️ A click OUTSIDE the link's run emits nothing.
///
/// Without this the test above passes for an implementation that fires
/// `meta_clicked` for any click anywhere on the label — which is the easiest
/// possible mistake here and completely useless behaviour.
#[test]
fn clicking_outside_the_url_run_emits_nothing() {
    let (mut tree, id) = label("go to [url=res://next.tscn]the link[/url] now");

    let src = "extends RichTextLabel\nvar got = \"\"\nfunc _on_meta(m):\n    self.got = m\n";
    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(src, id).unwrap()),
    );
    tree.connect_signal(
        id,
        "meta_clicked",
        Connection::new(id.object_id(), "_on_meta"),
    );

    // x = 1 is inside the label and inside the leading "go to " run, which
    // carries no meta.
    click(&mut tree, 1.0, 5.0);

    assert_eq!(
        script_var(&tree, id, "got"),
        Variant::String("".into()),
        "only the [url] run is clickable; the rest of the label is not a link"
    );
}

/// The tag itself is never drawn — the payload is metadata, not text.
#[test]
fn the_url_tag_does_not_leak_into_the_drawn_text() {
    let (mut tree, id) = label("a[url=xyz]b[/url]c");
    let joined = drawn(&mut tree, id).join("");

    assert_eq!(
        joined, "abc",
        "the run text is the link's label, not its target"
    );
}

// ---------------------------------------------------------------------------
// visible_characters / visible_ratio
// ---------------------------------------------------------------------------

/// `visible_characters = 3` draws only the first three glyphs.
///
/// ⚠️ The budget counts PARSED glyphs, so tags do not consume it — a label whose
/// first tag ate the allowance would reveal nothing and look like a blank
/// control.
#[test]
fn visible_characters_trims_the_drawn_glyphs() {
    let (mut tree, id) = label("[color=#ff0000]abcdef[/color]");

    assert_eq!(
        drawn(&mut tree, id).join(""),
        "abcdef",
        "unset means fully visible"
    );

    tree.get_node_mut(id)
        .unwrap()
        .set_property("visible_characters", Variant::Int(3));
    assert_eq!(
        drawn(&mut tree, id).join(""),
        "abc",
        "only the first three glyphs are drawn; the 6-char tag before them must \
         not have consumed the budget"
    );

    tree.get_node_mut(id)
        .unwrap()
        .set_property("visible_characters", Variant::Int(-1));
    assert_eq!(
        drawn(&mut tree, id).join(""),
        "abcdef",
        "-1 means all, as in Godot"
    );
}

/// The budget spans runs rather than restarting at each one, and truncates a
/// partially-visible run instead of dropping it.
///
/// A per-run budget would reveal text a whole run at a time, which is visibly
/// wrong the moment any tag splits the line.
#[test]
fn the_visible_budget_spans_runs_and_truncates_mid_run() {
    let (mut tree, id) = label("ab[color=#00ff00]cdef[/color]");
    tree.get_node_mut(id)
        .unwrap()
        .set_property("visible_characters", Variant::Int(4));

    assert_eq!(
        drawn(&mut tree, id).join(""),
        "abcd",
        "2 glyphs from the first run and 2 from the second — not 'ab' (budget \
         restarted) and not 'abcdef' (budget ignored past the first run)"
    );
}

/// `visible_ratio` expresses the same limit as a fraction.
#[test]
fn visible_ratio_reveals_a_fraction_of_the_text() {
    let (mut tree, id) = label("abcdefgh");
    tree.get_node_mut(id)
        .unwrap()
        .set_property("visible_ratio", Variant::Float(0.5));

    assert_eq!(drawn(&mut tree, id).join(""), "abcd", "half of 8 glyphs");
}

// ---------------------------------------------------------------------------
// ClassDB surface
// ---------------------------------------------------------------------------

/// The meta signals are **discoverable**, not merely emitted.
///
/// ⚠️ These are two independent things, and only one of them was missing.
/// `emit_signal` dispatches to connections and never consults ClassDB — which is
/// why `meta_clicked` already reached a connected handler in the tests above
/// while `has_signal("meta_clicked")` answered false. The declaration is what
/// the inspector's signal dock, `.tscn` `[connection]` validation and
/// `Object.has_signal` all read, so without it the signal WORKS and CANNOT BE
/// FOUND — the sort of half-present feature that reads as a bug in the editor
/// long before anyone suspects the registration.
#[test]
fn the_richtextlabel_signals_are_declared_in_classdb() {
    gdobject::class_db::register_2d_classes();

    for sig in [
        "meta_clicked",
        "meta_hover_started",
        "meta_hover_ended",
        "finished",
    ] {
        assert!(
            gdobject::class_db::class_has_signal("RichTextLabel", sig),
            "RichTextLabel must declare {sig} (doc/classes/RichTextLabel.xml)"
        );
    }
}

/// The property surface the inspector reads.
#[test]
fn the_richtextlabel_property_surface_is_registered() {
    gdobject::class_db::register_2d_classes();
    let props = gdobject::class_db::get_property_list("RichTextLabel");

    for name in [
        "bbcode_enabled",
        "text",
        "fit_content",
        "scroll_following",
        "visible_characters",
        "visible_ratio",
    ] {
        assert!(
            props.iter().any(|p| p.name == name),
            "RichTextLabel must expose {name}; got {:?}",
            props.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
}

// ---------------------------------------------------------------------------
// [u] / [s]
// ---------------------------------------------------------------------------

/// Every line the label draws, as `(from, to)` pairs.
fn drawn_lines(tree: &mut SceneTree, id: NodeId) -> Vec<(Vector2, Vector2)> {
    let mut vp = Viewport::new(300, 40, Color::BLACK);
    let theme = Theme::default_theme();
    gdscene::control_render::render_control_tree(
        tree,
        id,
        &theme,
        &mut vp,
        Vector2::new(300.0, 40.0),
    );
    vp.get_sorted_items()
        .iter()
        .flat_map(|it| it.commands.clone())
        .filter_map(|c| match c {
            DrawCommand::DrawLine { from, to, .. } => Some((from, to)),
            _ => None,
        })
        .collect()
}

/// `[u]` draws a rule UNDER the run; `[s]` draws one THROUGH it.
///
/// ⚠️ Asserted by vertical position, not merely by "a line exists". Underline
/// and strikethrough differ only in where the rule sits, so a test that counts
/// lines passes with the two swapped — which is exactly the mistake worth
/// catching.
#[test]
fn underline_and_strikethrough_draw_rules_at_different_heights() {
    let (mut tree, id) = label("[u]under[/u]");
    let under = drawn_lines(&mut tree, id);
    assert_eq!(under.len(), 1, "one underline rule, got {under:?}");

    let (mut tree2, id2) = label("[s]struck[/s]");
    let struck = drawn_lines(&mut tree2, id2);
    assert_eq!(struck.len(), 1, "one strike rule, got {struck:?}");

    assert!(
        under[0].0.y > struck[0].0.y,
        "the underline must sit BELOW the strikethrough — underline {} vs \
         strike {}. They differ only in height, so swapping them is invisible \
         to a test that only counts rules",
        under[0].0.y,
        struck[0].0.y,
    );

    // Each rule spans its run horizontally rather than collapsing to a point.
    assert!(
        under[0].1.x > under[0].0.x,
        "the rule must span the run's width, got {:?}",
        under[0]
    );
}

/// `[b]` is OVER-STRUCK; `[i]` still has nothing to render; neither draws a rule.
///
/// ⚠️ This test previously asserted that `[b]` and `[i]` were both *stripped*,
/// on the grounds that the built-in 5x7 bitmap font has no weight or slant
/// variant so a bold flag would have nothing to render (pat-s3rrv.74).
///
/// Half of that was right. There is no bold FACE to select — but weight can
/// still be faked by over-striking the run one pixel across, which thickens
/// every stem by exactly one pixel. So `[b]` now renders, and only `[i]` stays
/// unrepresentable: a bitmap glyph cannot be sheared without resampling it.
///
/// The original warning still stands and is still pinned below: neither tag may
/// be "fixed" by emitting a RULE. A bold word with a line under it is worse
/// than a plain one.
#[test]
fn bold_is_overstruck_italic_is_not_and_neither_draws_a_rule() {
    let (mut tree, id) = label("[b]bold[/b] [i]it[/i]");
    let runs = drawn(&mut tree, id);

    // The tag text itself never reaches the glyphs.
    assert!(
        !runs.join("").contains('['),
        "the tags are stripped from the rendered text, got {runs:?}"
    );
    assert!(
        runs.iter().any(|r| r.contains("bold")),
        "the bold run's text survives, got {runs:?}"
    );
    assert!(
        runs.iter().any(|r| r.contains("it")),
        "the italic run's text survives, got {runs:?}"
    );

    // ⚠️ The over-strike: the bold run is emitted TWICE, the italic run once.
    // Counting both in one test is what makes it meaningful — an implementation
    // that doubled every run would satisfy the bold half alone.
    let bold_runs = runs.iter().filter(|r| r.trim() == "bold").count();
    let italic_runs = runs.iter().filter(|r| r.trim() == "it").count();
    assert_eq!(
        bold_runs, 2,
        "a [b] run is drawn twice — once plain, once over-struck one pixel \
         right — to fake weight the bitmap font has no variant for; got {runs:?}"
    );
    assert_eq!(
        italic_runs, 1,
        "an [i] run is drawn ONCE: a bitmap glyph cannot be sheared, so slant \
         stays unrepresentable and must not be faked by doubling; got {runs:?}"
    );

    // The original guard, unchanged.
    assert!(
        drawn_lines(&mut tree, id).is_empty(),
        "no rule is drawn for weight or slant — inventing a decoration would be \
         worse than omitting it"
    );
}

/// Nested `[u]` survives the inner closer.
///
/// Tracked as a depth counter rather than a flag, so `[u]a[u]b[/u]c[/u]` keeps
/// `c` underlined — with a bool the inner `[/u]` would clear the outer tag and
/// the tail would silently lose its rule.
#[test]
fn nested_underline_survives_the_inner_closer() {
    let (mut tree, id) = label("[u]a[u]b[/u]c[/u]d");
    let lines = drawn_lines(&mut tree, id);

    assert_eq!(
        lines.len(),
        3,
        "runs a, b and c are all underlined and d is not, so three rules — \
         got {lines:?}"
    );
}

/// ⚠️ A RichTextLabel must NOT default to `mouse_filter = IGNORE`.
///
/// Godot sets IGNORE in `label.cpp:1477`. `rich_text_label.cpp` has **no**
/// `set_mouse_filter` call anywhere, so it keeps Control's `STOP`. Patina had
/// generalised Label's setting to it — a one-word difference whose consequence
/// is not cosmetic: an IGNORE'd control never wins the hit test, so every
/// `[url=…]` link is unclickable and `meta_clicked` can never fire, with no
/// error anywhere to say why. The link parses, draws, and is simply dead.
///
/// This is the bug the click test above actually caught.
#[test]
fn a_richtextlabel_does_not_ignore_the_mouse() {
    let (tree, id) = label("plain text");

    assert_ne!(
        gdscene::control::get_mouse_filter(&tree, id),
        gdscene::control::MouseFilter::Ignore,
        "RichTextLabel inherits Control's STOP; only Label sets IGNORE \
         (label.cpp:1477). Ignoring the mouse makes [url] links unclickable."
    );
}

// ---------------------------------------------------------------------------
// meta hover
// ---------------------------------------------------------------------------

/// Records `started:<meta>` / `ended:<meta>` in emission order.
fn watch_hover(tree: &mut SceneTree, id: NodeId) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
    use gdobject::signal::Connection;
    let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    for (signal, tag) in [
        ("meta_hover_started", "started"),
        ("meta_hover_ended", "ended"),
    ] {
        let sink = log.clone();
        let tag = tag.to_string();
        tree.connect_signal(
            id,
            signal,
            Connection::with_callback(
                gdcore::id::ObjectId::from_raw(if tag == "started" { 91 } else { 92 }),
                "on_hover",
                move |args| {
                    let m = match args.first() {
                        Some(Variant::String(s)) => s.clone(),
                        other => panic!("meta signals carry a payload, got {other:?}"),
                    };
                    sink.lock().unwrap().push(format!("{tag}:{m}"));
                    Variant::Nil
                },
            ),
        );
    }
    log
}

fn motion(tree: &mut SceneTree, x: f32) {
    tree.push_input(&InputEvent::MouseMotion {
        position: Vector2::new(x, 5.0),
        relative: Vector2::new(1.0, 0.0),
    });
}

/// Entering, moving within, and leaving a link.
///
/// ⚠️ The middle step is the one that matters: moving WITHIN a link must emit
/// nothing. Without that guard every motion sample over a link re-fires the
/// pair, which turns a hover into a stream of hundreds of signals per second —
/// a bug that looks like nothing at all until a handler does real work.
#[test]
fn hovering_a_link_starts_and_ends_once() {
    let (mut tree, id) = label("go to [url=res://a.tscn]link[/url] now");
    let log = watch_hover(&mut tree, id);

    let regions = gdscene::control_render::richtext_meta_regions(
        &tree,
        id,
        gdscene::control_render::RICHTEXT_DEFAULT_FONT_SIZE,
        gdcore::math::Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(300.0, 40.0)),
    );
    let (rect, _) = regions[0].clone();

    motion(&mut tree, rect.position.x + 2.0);
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["started:res://a.tscn"],
        "entering the run starts the hover"
    );

    motion(&mut tree, rect.position.x + rect.size.x - 2.0);
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["started:res://a.tscn"],
        "moving WITHIN the same run must emit nothing further"
    );

    motion(&mut tree, 1.0);
    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["started:res://a.tscn", "ended:res://a.tscn"],
        "leaving reports the run that was LEFT"
    );
}

/// Sliding straight from one link to another reports both, in order.
///
/// ⚠️ `meta_hover_ended` must carry the payload of the run being **left**
/// (`rich_text_label.cpp:3055-3072`). Reporting the newly entered one instead is
/// invisible with a single link and wrong the moment there are two — which is
/// why the fixture has two.
#[test]
fn sliding_between_links_ends_the_old_before_starting_the_new() {
    let (mut tree, id) = label("[url=first]aaaa[/url]-[url=second]bbbb[/url]");
    let log = watch_hover(&mut tree, id);

    let regions = gdscene::control_render::richtext_meta_regions(
        &tree,
        id,
        gdscene::control_render::RICHTEXT_DEFAULT_FONT_SIZE,
        gdcore::math::Rect2::new(Vector2::new(0.0, 0.0), Vector2::new(300.0, 40.0)),
    );
    assert_eq!(regions.len(), 2, "two links, got {regions:?}");

    motion(&mut tree, regions[0].0.position.x + 2.0);
    motion(&mut tree, regions[1].0.position.x + 2.0);

    assert_eq!(
        log.lock().unwrap().clone(),
        vec!["started:first", "ended:first", "started:second"],
        "the old link's END carries ITS payload and precedes the new link's START"
    );
}
