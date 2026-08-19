//! pat-s3rrv.74: a GDScript can build a RichTextLabel's contents.
//!
//! Godot ground truth: `doc/classes/RichTextLabel.xml` — `append_text` "parses
//! [param bbcode] and adds tags to the tag stack"; `add_text` "adds **raw
//! non-BBCode-parsed** text"; `get_parsed_text` returns the text with markup
//! removed (`rich_text_label.cpp:7183` concatenates the text items). `[lb]` and
//! `[rb]` are Godot's literal-bracket escapes (`rich_text_label.cpp:5621-5626`).
//!
//! ⚠️ Before this, a script could only replace the whole `bbcode_text` string —
//! so nothing could append a line to a log or a dialogue box, which is most of
//! what a RichTextLabel exists for. `append_text`, `add_text`, `clear`,
//! `get_parsed_text` and `get_total_character_count` had **zero** non-test hits.
//!
//! Every call below uses the **bare** spelling (`append_text(...)` from a script
//! on the label itself). That is what a script attached to the control writes,
//! and it resolves through `call_self_func_or_native` rather than the
//! interpreter's builtin match — a binding wired only to the latter is
//! unreachable from it.

use gdcore::math::{Color, Vector2};
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

/// A RichTextLabel carrying `bbcode`, with `src` attached as its script.
fn labelled(bbcode: &str, src: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("R", "RichTextLabel");
    n.set_property("bbcode_enabled", Variant::Bool(true));
    n.set_property("bbcode_text", Variant::String(bbcode.to_owned()));
    let id = tree.add_child(root, n).unwrap();
    gdscene::control::set_position(&mut tree, id, Vector2::new(0.0, 0.0));
    gdscene::control::set_size(&mut tree, id, Vector2::new(300.0, 40.0));
    tree.attach_script(
        id,
        Box::new(GDScriptNodeInstance::from_source(src, id).expect("script parses")),
    );
    (tree, id)
}

/// Runs `run()` and asserts it did not raise.
///
/// `call_script_method` returns `None` when the script raised, which is how an
/// unresolved method name presents: `UndefinedFunction` aborts the handler at
/// that statement and every later line silently does not run.
fn run(tree: &mut SceneTree, id: NodeId) {
    assert!(
        tree.call_script_method(id, "run", &[]).is_some(),
        "run() raised — most likely the method name did not resolve at all"
    );
}

fn script_var(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    tree.get_script(id)
        .expect("script attached")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

fn drawn(tree: &mut SceneTree, id: NodeId) -> String {
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

// ---------------------------------------------------------------------------

/// `append_text` adds parsed BBCode, and it reaches the next frame's draw.
#[test]
fn append_text_appends_parsed_bbcode_and_reaches_the_draw() {
    let (mut tree, id) = labelled(
        "start ",
        "extends RichTextLabel\nfunc run():\n    append_text(\"[color=#ff0000]red[/color]\")\n",
    );

    run(&mut tree, id);

    assert_eq!(
        drawn(&mut tree, id),
        "start red",
        "the appended run must be parsed (the tag stripped) and drawn after the \
         existing text"
    );
}

/// ⚠️ `add_text` adds RAW text: brackets in it are NOT markup.
///
/// This is the assertion that matters. Appending user data unescaped lets a
/// stray `[` — in a chat line, a filename, a stack trace — be read as a tag,
/// which silently swallows the rest of the string and can inject styling. The
/// brackets round-trip through Godot's own `[lb]`/`[rb]` escapes.
#[test]
fn add_text_treats_brackets_as_literal_text() {
    let (mut tree, id) = labelled(
        "",
        "extends RichTextLabel\nvar seen = \"\"\nfunc run():\n    add_text(\"a[color=#ff0000]b\")\n    self.seen = get_parsed_text()\n",
    );

    run(&mut tree, id);

    assert_eq!(
        script_var(&tree, id, "seen"),
        Variant::String("a[color=#ff0000]b".into()),
        "raw text must survive intact — if the brackets were parsed, the tag \
         would vanish and only 'ab' would remain"
    );
    assert_eq!(
        drawn(&mut tree, id),
        "a[color=#ff0000]b",
        "and it must DRAW the brackets rather than applying them"
    );
}

/// ⚠️ Nested brackets round-trip too — the case that catches double-escaping.
///
/// Escaping `[` emits a `]` of its own, so escaping the two characters with two
/// chained `replace` calls mangles whichever escape was written first: `[lb]`
/// becomes `[lb[rb]`, an unknown tag that is then stripped, and the user's
/// bracket vanishes. No ordering of two replaces avoids it, because each emits
/// both characters the other rewrites — it has to be one pass.
///
/// `[[]]` is the shortest input that fails under either ordering, so it is the
/// one worth pinning.
#[test]
fn add_text_round_trips_nested_brackets() {
    let (mut tree, id) = labelled(
        "",
        "extends RichTextLabel\nvar seen = \"\"\nfunc run():\n    add_text(\"[[]]\")\n    self.seen = get_parsed_text()\n",
    );

    run(&mut tree, id);

    assert_eq!(
        script_var(&tree, id, "seen"),
        Variant::String("[[]]".into()),
        "every bracket survives; a chained-replace escape loses the first one"
    );
}

/// `clear()` empties the label.
#[test]
fn clear_empties_the_label() {
    let (mut tree, id) = labelled(
        "[color=#00ff00]green[/color]",
        "extends RichTextLabel\nfunc run():\n    clear()\n",
    );

    run(&mut tree, id);

    assert_eq!(drawn(&mut tree, id), "", "nothing is drawn after clear()");
}

/// `get_parsed_text` returns the text with markup removed.
#[test]
fn get_parsed_text_strips_the_markup() {
    let (mut tree, id) = labelled(
        "a[color=#ff0000]b[/color][u]c[/u]",
        "extends RichTextLabel\nvar seen = \"\"\nfunc run():\n    self.seen = get_parsed_text()\n",
    );

    run(&mut tree, id);

    assert_eq!(
        script_var(&tree, id, "seen"),
        Variant::String("abc".into()),
        "every tag is removed and every character kept"
    );
}

/// `get_total_character_count` counts parsed characters, not markup.
///
/// ⚠️ Counting the raw string instead would make the count grow with the
/// styling, which is exactly wrong for its main use: it is the denominator a
/// typewriter effect drives `visible_characters` against, so an inflated total
/// would stall the reveal partway with no text left to show.
#[test]
fn get_total_character_count_ignores_markup() {
    let (mut tree, id) = labelled(
        "[color=#ff0000]abc[/color]",
        "extends RichTextLabel\nvar n = -1\nfunc run():\n    self.n = get_total_character_count()\n",
    );

    run(&mut tree, id);

    let n = match script_var(&tree, id, "n") {
        Variant::Int(i) => i,
        Variant::Float(f) => f as i64,
        other => panic!("expected a number, got {other:?}"),
    };
    assert_eq!(
        n, 3,
        "three visible characters — the 20-odd characters of markup must not count"
    );
}

/// ⚠️ A non-RichTextLabel does NOT answer these methods.
///
/// The arms are guarded on the class, and this is what proves the guard rather
/// than assuming it: a Panel calling `get_parsed_text()` must fail to resolve
/// the name, which surfaces as the handler raising. Without the guard the arm
/// would answer for every node in the scene and shadow the generic node-method
/// table beneath it.
#[test]
fn a_plain_control_does_not_answer_the_richtext_methods() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let panel = tree.add_child(root, Node::new("P", "Panel")).unwrap();
    tree.attach_script(
        panel,
        Box::new(
            GDScriptNodeInstance::from_source(
                "extends Panel
var seen = \"untouched\"
func run():
    self.seen = get_parsed_text()
",
                panel,
            )
            .unwrap(),
        ),
    );

    assert!(
        tree.call_script_method(panel, "run", &[]).is_none(),
        "`get_parsed_text()` must not resolve on a Panel — an arm that answered \
         here would shadow the generic node-method table for every class"
    );
    assert_eq!(
        script_var(&tree, panel, "seen"),
        Variant::String("untouched".into()),
        "and the assignment never happens, because the call raised"
    );
}
