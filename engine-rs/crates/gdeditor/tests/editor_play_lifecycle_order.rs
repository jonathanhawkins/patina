//! pat-0q2kx.46 acceptance: the editor's Play flow must deliver Godot's
//! lifecycle sequence — `_enter_tree` parent-first, `_ready` child-first, each
//! exactly once, autoloads before the main scene.
//!
//! `gdscene::lifecycle::LifecycleManager::enter_tree` already implemented this
//! correctly and the headless runner used it, but `api_runtime_play` did not: it
//! called `process_script_ready` per scripted node in `all_nodes_in_tree_order()`
//! — **parent-first**, the reverse of Godot — and never dispatched `_enter_tree`,
//! `_notification(READY)` or `tree_entered` at all. A script attached to a node
//! that was already inside the tree therefore never saw `_enter_tree` in the
//! editor, while the same scene run through the runner did.
//!
//! The sequence is recorded the way the bead specifies — from inside the scripts
//! themselves, via an autoload each node reports to. That makes the probe
//! double-duty: every `Recorder.record(...)` call from a scene script is also a
//! live cross-reference to `/root/Recorder`, so a run that recorded anything at
//! all proves the autoload was reachable from the main scene's callbacks.

use gdeditor::editor_server::{build_play_tree, EditorState};
use gdobject::notification::{Notification, NOTIFICATION_ENTER_TREE, NOTIFICATION_READY};
use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// Reports each of its own callbacks and offers `record()` to the scene's scripts.
///
/// It appends to its own `seq` with a bare call rather than through the
/// `Recorder` global: a script that is currently running is checked out of the
/// tree's script arena, so a re-entrant dispatch back to itself would find
/// nothing to call.
const RECORDER_GD: &str = r#"extends Node

var seq = ""

func record(tag):
	seq += tag + "|"

func _enter_tree():
	record("Recorder:enter")

func _ready():
	record("Recorder:ready")
"#;

/// A scene node's script. `Recorder` resolves as an autoload global (pat-nzgdu.70)
/// and the call lands on that node's script instance.
fn node_gd(tag: &str) -> String {
    format!(
        "extends Node\n\nfunc _enter_tree():\n\tRecorder.record(\"{tag}:enter\")\n\nfunc _ready():\n\tRecorder.record(\"{tag}:ready\")\n"
    )
}

/// A project on disk with `Recorder` declared as an autoload, plus one script per
/// scene node. The `*` prefix is Godot's "enabled singleton" marker.
fn write_project(dir: &std::path::Path, tags: &[&str]) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\n\n[autoload]\nRecorder=\"*res://recorder.gd\"\n",
    )
    .expect("project.godot");
    std::fs::write(dir.join("recorder.gd"), RECORDER_GD).expect("recorder.gd");
    for tag in tags {
        std::fs::write(
            dir.join(format!("{}.gd", tag.to_ascii_lowercase())),
            node_gd(tag),
        )
        .expect("node script");
    }
}

/// A three-level chain `A -> B -> C`, each node carrying its own script.
fn three_level_scene() -> SceneTree {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut parent = root;
    for tag in ["A", "B", "C"] {
        let mut node = Node::new(tag, "Node");
        node.set_property(
            "_script_path",
            Variant::String(format!("res://{}.gd", tag.to_ascii_lowercase())),
        );
        parent = tree.add_child(parent, node).expect("add scene node");
    }
    let _ = parent;
    tree
}

fn node_named(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| tree.get_node(*id).map(|n| n.name()) == Some(name))
        .unwrap_or_else(|| panic!("no node named {name} in the played tree"))
}

/// The recorded sequence, split on `|`.
fn recorded(tree: &SceneTree) -> Vec<String> {
    let recorder = node_named(tree, "Recorder");
    let script = tree
        .get_script(recorder)
        .expect("the Recorder autoload must have its script attached");
    match script.get_property("seq") {
        Some(Variant::String(s)) => s
            .split('|')
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect(),
        other => panic!("Recorder.seq must be a String, got {other:?}"),
    }
}

fn position_of(seq: &[String], tag: &str) -> usize {
    seq.iter()
        .position(|s| s == tag)
        .unwrap_or_else(|| panic!("{tag} never fired; recorded sequence was {seq:?}"))
}

fn count_of(log: &[Notification], what: Notification) -> usize {
    log.iter().filter(|n| **n == what).count()
}

/// **The acceptance test.** Playing a three-level scene records the exact Godot
/// sequence: every `_enter_tree` parent-first, every `_ready` child-first, one of
/// each, with the autoload fully ready before the scene enters.
#[test]
fn editor_play_lifecycle_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_project(dir.path(), &["A", "B", "C"]);

    let mut state = EditorState::new(three_level_scene());
    state
        .texture_cache
        .set_project_root(dir.path().to_string_lossy().to_string());

    let played = build_play_tree(&mut state);
    let seq = recorded(&played);

    // The headline contract, stated as the whole sequence so a reordering shows
    // up as a diff rather than as one failed inequality.
    let expected = [
        "Recorder:enter",
        "Recorder:ready",
        "A:enter",
        "B:enter",
        "C:enter",
        "C:ready",
        "B:ready",
        "A:ready",
    ];
    assert_eq!(
        seq, expected,
        "editor Play must dispatch Godot's lifecycle order",
    );

    // Spelled out, so a failure names the specific rule that broke.
    assert!(
        position_of(&seq, "A:enter") < position_of(&seq, "B:enter")
            && position_of(&seq, "B:enter") < position_of(&seq, "C:enter"),
        "_enter_tree must run parent-first: {seq:?}",
    );
    assert!(
        position_of(&seq, "C:ready") < position_of(&seq, "B:ready")
            && position_of(&seq, "B:ready") < position_of(&seq, "A:ready"),
        "_ready must run child-first (deepest node first): {seq:?}",
    );
    // The bug this replaces: `_ready` was dispatched in tree order, so the
    // shallowest node readied first and a parent's `_ready` could not see work
    // its children do in theirs.
    assert!(
        position_of(&seq, "A:ready") > position_of(&seq, "C:ready"),
        "the root's _ready must run last, not first: {seq:?}",
    );
    // Each callback exactly once — no node is entered or readied twice.
    for tag in expected {
        assert_eq!(
            seq.iter().filter(|s| s.as_str() == tag).count(),
            1,
            "{tag} must fire exactly once: {seq:?}",
        );
    }
    // Godot's boot order: the autoload is live before the main scene starts, and
    // every entry after `Recorder:ready` was recorded *through* the autoload —
    // so the scene's callbacks could reach `/root/Recorder`.
    assert!(
        position_of(&seq, "Recorder:ready") < position_of(&seq, "A:enter"),
        "an autoload must be fully _ready before the main scene enters: {seq:?}",
    );
}

/// The node-level notifications match the script callbacks: one `ENTER_TREE` and
/// one `READY` per node.
///
/// Assembling a subtree under a live root enters each node as it is inserted, so
/// without the rewind the real dispatch is a *second* pass and every node logs
/// both notifications twice. The scripts cannot see that duplication — they are
/// attached after the premature pass — which is exactly why it needs its own
/// assertion.
#[test]
fn play_notifies_each_node_exactly_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_project(dir.path(), &["A", "B", "C"]);

    let mut state = EditorState::new(three_level_scene());
    state
        .texture_cache
        .set_project_root(dir.path().to_string_lossy().to_string());
    let played = build_play_tree(&mut state);

    for name in ["Recorder", "A", "B", "C"] {
        let id = node_named(&played, name);
        let node = played.get_node(id).expect("node");
        let log = node.notification_log();
        assert_eq!(
            count_of(log, NOTIFICATION_ENTER_TREE),
            1,
            "{name} must receive ENTER_TREE exactly once, log was {log:?}",
        );
        assert_eq!(
            count_of(log, NOTIFICATION_READY),
            1,
            "{name} must receive READY exactly once, log was {log:?}",
        );
        assert!(
            node.is_inside_tree(),
            "{name} must be left inside the tree after the dispatch",
        );
        assert!(
            node.is_ready(),
            "{name} must be left ready after the dispatch"
        );
    }
}

/// The runner's autoload boot path has the same duplication and is fixed by the
/// same rewind, so it is pinned here rather than left to the editor case.
///
/// `boot_autoloads_from_dir` instantiates each autoload with `add_child` and then
/// dispatches lifecycle explicitly; `instantiate_one` attaches the script only
/// after the node is added, so the first pass is scriptless and the second is the
/// one user code sees.
#[test]
fn autoload_boot_notifies_each_node_exactly_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_project(dir.path(), &[]);

    let mut tree = SceneTree::new();
    let result = gdscene::autoload::boot_autoloads_from_dir(&mut tree, dir.path());
    assert_eq!(
        result.nodes.len(),
        1,
        "the Recorder autoload must be instantiated",
    );

    let id = node_named(&tree, "Recorder");
    let log = tree.get_node(id).expect("node").notification_log();
    assert_eq!(
        count_of(log, NOTIFICATION_ENTER_TREE),
        1,
        "an autoload must receive ENTER_TREE exactly once, log was {log:?}",
    );
    assert_eq!(
        count_of(log, NOTIFICATION_READY),
        1,
        "an autoload must receive READY exactly once, log was {log:?}",
    );

    // And its own script still ran both callbacks, once each, in order.
    let script = tree.get_script(id).expect("script attached");
    assert_eq!(
        script.get_property("seq"),
        Some(Variant::String(
            "Recorder:enter|Recorder:ready|".to_string()
        )),
        "the autoload's script must see _enter_tree then _ready, once each",
    );
}
