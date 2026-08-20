//! pat-s3rrv.74: RichTextLabel's `push_*` / `pop` tag stack.
//!
//! Godot builds rich text from script with a STACK of open tags —
//! `push_color(RED); add_text("hi"); pop()` — which is how a log or chat box
//! composes a line without hand-writing BBCode (`RichTextLabel.xml`).
//!
//! ⚠️ A repo-wide grep for `push_color` in `gdscene/src` returned ZERO: none of
//! the push/pop API existed, so every one of those calls raised
//! `UndefinedFunction` and aborted the handler at that line.
//!
//! 🔑 These emit into the SAME BBCode source string `append_text` appends to,
//! which is why no parallel document model is needed: the draw arm re-parses
//! that string every frame, so a push is visible on the next frame like any
//! other edit. Asserting on the source is therefore asserting on what will be
//! drawn, not on a bookkeeping side-channel.
//!
//! ⚠️ Only tags `parse_bbcode` actually understands are emitted. A tag it does
//! not recognise is STRIPPED, so an unsupported push would look like it worked
//! — the call returns, the text still renders — while doing nothing. That is
//! why there is no `push_bold` here: `[b]` is parsed into a run attribute and
//! over-struck by the draw arm (pat-s3rrv.74), but the push/pop stack itself is
//! still unimplemented. The built-in 5x7
//! bitmap font has no weight variant to render it with.

use gdcore::math::{Color, Vector2};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn labelled(src: &str) -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut n = Node::new("R", "RichTextLabel");
    n.set_property("bbcode_enabled", Variant::Bool(true));
    n.set_property("bbcode_text", Variant::String(String::new()));
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
/// `call_script_method` answers `None` when the script raised, which is exactly
/// how a missing method presents: `UndefinedFunction` aborts the handler at that
/// statement and every later line silently does not run. Without this check a
/// test for an unimplemented API reads as "the output was empty".
fn run(tree: &mut SceneTree, id: NodeId) {
    assert!(
        tree.call_script_method(id, "run", &[]).is_some(),
        "run() raised — most likely a push_*/pop method name did not resolve"
    );
}

/// The label's BBCode source, which is what the draw arm parses.
fn source(tree: &SceneTree, id: NodeId) -> String {
    match tree.get_node(id).map(|n| n.get_property("bbcode_text")) {
        Some(Variant::String(s)) => s,
        _ => String::new(),
    }
}

#[test]
fn push_color_then_pop_wraps_the_text_in_a_color_tag() {
    let (mut tree, id) = labelled(
        "\
extends RichTextLabel
func run():
    push_color(Color(1, 0, 0, 1))
    add_text(\"hi\")
    pop()
",
    );
    run(&mut tree, id);

    assert_eq!(
        source(&tree, id),
        "[color=#ff0000ff]hi[/color]",
        "push_color must open a colour tag, add_text must land inside it, and \
         pop must close it"
    );
}

/// 🔑 THE STACK PROPERTY: `pop` closes the MOST RECENT push, not the first.
///
/// A implementation that remembered only one open tag, or closed them in
/// insertion order, passes the single-tag test above and fails here.
#[test]
fn pop_closes_the_most_recent_push_first() {
    let (mut tree, id) = labelled(
        "\
extends RichTextLabel
func run():
    push_color(Color(0, 0, 1, 1))
    push_underline()
    add_text(\"x\")
    pop()
    add_text(\"y\")
    pop()
",
    );
    run(&mut tree, id);

    assert_eq!(
        source(&tree, id),
        "[color=#0000ffff][u]x[/u]y[/color]",
        "the first pop must close the underline (the innermost open tag) and \
         leave the colour open, so `y` is still coloured"
    );
}

#[test]
fn pop_all_closes_every_open_tag() {
    let (mut tree, id) = labelled(
        "\
extends RichTextLabel
func run():
    push_color(Color(0, 1, 0, 1))
    push_underline()
    push_strikethrough()
    add_text(\"z\")
    pop_all()
",
    );
    run(&mut tree, id);

    assert_eq!(
        source(&tree, id),
        "[color=#00ff00ff][u][s]z[/s][/u][/color]",
        "pop_all must unwind the whole stack innermost-first"
    );
}

/// `push_meta` is what makes a run clickable — the payload `meta_clicked`
/// carries. The parser models it as `[url=…]`.
#[test]
fn push_meta_emits_a_url_run() {
    let (mut tree, id) = labelled(
        "\
extends RichTextLabel
func run():
    push_meta(\"go\")
    add_text(\"link\")
    pop()
",
    );
    run(&mut tree, id);

    assert_eq!(
        source(&tree, id),
        "[url=go]link[/url]",
        "push_meta must open a url run carrying the meta payload"
    );
}

/// CONTROL: `pop` on an empty stack is a no-op, not a crash or a stray tag.
///
/// Godot forgives an unbalanced script rather than erroring, and a stray
/// `[/color]` in the source would corrupt every later run.
#[test]
fn pop_without_a_push_is_a_no_op() {
    let (mut tree, id) = labelled(
        "\
extends RichTextLabel
func run():
    add_text(\"plain\")
    pop()
    pop()
",
    );
    run(&mut tree, id);

    assert_eq!(
        source(&tree, id),
        "plain",
        "popping an empty stack must add nothing — a stray close tag would \
         corrupt the runs after it"
    );
}
