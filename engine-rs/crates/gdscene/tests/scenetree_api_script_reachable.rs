//! pat-0q2kx.80 — the SceneTree surface is reachable from GDScript.
//!
//! Godot scripts drive scene flow through SceneTree (`SceneTree.xml`): tweens,
//! scene changes, group queries and calls, pause, quit. Patina had every
//! implementation — a full tween engine ticked each frame, `change_scene_to_file`,
//! `get_nodes_in_group`, `call_group` — reachable only from Rust.
//!
//! ⚠️ Much of the dispatch had already been wired by the time I picked this bead
//! up, but nothing PINNED it: the whole surface could regress to "unreachable"
//! without a single test failing, which is the exact state the bead describes.
//! These drive the real `MainLoop`, so a pass means a script reached the engine,
//! not that a Rust helper exists.

use gdcore::math::Vector2;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

fn attach(tree: &mut SceneTree, id: gdscene::node::NodeId, src: &str) {
    let script = GDScriptNodeInstance::from_source(src, id).expect("script parses");
    tree.attach_script(id, Box::new(script));
}

fn prop(ml: &MainLoop, id: gdscene::node::NodeId, name: &str) -> Option<Variant> {
    ml.tree().get_script(id).and_then(|s| s.get_property(name))
}

/// **The acceptance test, part 1.** `create_tween()` + `tween_property` animates
/// the node across ticks.
///
/// ⚠️ Tweens read and write NODE properties (`scene_tree.rs` `process_tweens`
/// resolves the target through `nodes.get_mut`), not script variables. My first
/// version of this test tweened a script `var` and asserted on the script
/// instance, so it read a value the tween never touches and reported 0 forever.
/// The bead's criteria say `position`, and `position` is what this drives.
#[test]
fn scenetree_api_script_reachable() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Mover", "Node2D");
    // Seed a real starting value: the tweener samples the node's CURRENT value
    // as its start, and an unset property would sample `Nil`, which cannot
    // interpolate toward a Vector2.
    node.set_property("position", Variant::Vector2(Vector2::new(0.0, 0.0)));
    let id = tree.add_child(root, node).unwrap();
    attach(
        &mut tree,
        id,
        "\
extends Node2D

func _ready():
    var t = create_tween()
    t.tween_property(self, \"position\", Vector2(100, 0), 0.5)
",
    );

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(id);

    let x_of = |ml: &MainLoop| match ml.tree().get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v.x,
        other => panic!("position should be a Vector2, got {other:?}"),
    };

    assert_eq!(x_of(&ml), 0.0, "the tween starts at the node's current value");

    // Half the duration: partway, not finished. A RANGE rather than an exact
    // value, so the easing curve is not pinned by this test.
    for _ in 0..15 {
        ml.step(1.0 / 60.0);
    }
    let mid = x_of(&ml);
    assert!(
        mid > 0.0 && mid < 100.0,
        "the tween must be mid-flight after ~0.25s, got {mid}"
    );

    // Past the end: settled exactly on the target.
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    let end = x_of(&ml);
    assert!(
        (end - 100.0).abs() < 0.001,
        "the tween must finish exactly on its target, got {end}"
    );
}

/// **Part 2.** `get_nodes_in_group` sees the tagged nodes and `call_group`
/// invokes their script method.
#[test]
fn group_query_and_call_reach_scripts() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let caller = tree.add_child(root, Node::new("Caller", "Node")).unwrap();

    // ⚠️ add_child FIRST, then add_to_group: a node grouped before it is in the
    // tree is invisible to `get_nodes_in_group`.
    for name in ["E1", "E2"] {
        let e = tree.add_child(root, Node::new(name, "Node")).unwrap();
        tree.add_to_group(e, "enemies").expect("grouping succeeds");
        attach(
            &mut tree,
            e,
            "\
extends Node
var hp = 10

func take_damage(n):
    self.hp = self.hp - n
",
        );
    }
    let enemies: Vec<gdscene::node::NodeId> = tree.get_nodes_in_group("enemies");
    assert_eq!(enemies.len(), 2, "fixture sanity: two grouped nodes");

    attach(
        &mut tree,
        caller,
        "\
extends Node
var found = 0

func _ready():
    var list = get_tree().get_nodes_in_group(\"enemies\")
    self.found = list.size()
    get_tree().call_group(\"enemies\", \"take_damage\", 3)
",
    );

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(caller);

    assert_eq!(
        prop(&ml, caller, "found"),
        Some(Variant::Int(2)),
        "get_nodes_in_group must return the tagged nodes to the script"
    );
    for e in enemies {
        assert_eq!(
            prop(&ml, e, "hp"),
            Some(Variant::Int(7)),
            "call_group must invoke each member's script method"
        );
    }
}

/// **Part 3.** `change_scene_to_file` swaps the running scene at frame end, not
/// mid-call — Godot defers it so the current frame finishes on a stable tree.
#[test]
fn change_scene_to_file_swaps_at_frame_end() {
    let dir = tempfile::tempdir().expect("tempdir");
    let next = dir.path().join("b.tscn");
    std::fs::write(
        &next,
        "[gd_scene format=3]\n\n[node name=\"SceneB\" type=\"Node2D\"]\n",
    )
    .unwrap();

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("SceneA", "Node")).unwrap();
    // The running scene, as every boot path records it: `change_scene_*`
    // frees `current_scene` only (Godot), so a bare root child would survive.
    tree.set_current_scene(id, None);
    // `change_scene_to_file` resolves through `gdcore::resolve_path`, so the
    // script is handed an absolute path rather than a `res://` one.
    let src = format!(
        "extends Node\n\nfunc go():\n    get_tree().change_scene_to_file(\"{}\")\n",
        next.to_string_lossy().replace('\\', "\\\\")
    );
    attach(&mut tree, id, &src);

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().call_script_method(id, "go", &[]);

    // ⚠️ Still the old scene immediately after the call: the swap is queued.
    assert!(
        ml.tree().get_node(id).is_some(),
        "the scene must NOT swap inside the call — Godot defers it to frame end"
    );

    ml.step(1.0 / 60.0);

    let names: Vec<String> = {
        let tree = ml.tree();
        let mut ids = Vec::new();
        tree.collect_subtree_top_down(tree.root_id(), &mut ids);
        ids.iter()
            .filter_map(|i| tree.get_node(*i).map(|n| n.name().to_string()))
            .collect()
    };
    assert!(
        names.iter().any(|n| n == "SceneB"),
        "the queued scene must be live after one frame: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "SceneA"),
        "the previous scene must be gone: {names:?}"
    );
}

/// Pause is reachable both ways. Godot's setter is the irregular `set_pause`,
/// paired with `is_paused` — not `set_paused`, which is why a grep for the
/// symmetric name finds nothing.
#[test]
fn pause_state_round_trips_through_scripts() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let id = tree.add_child(root, Node::new("P", "Node")).unwrap();
    attach(
        &mut tree,
        id,
        "\
extends Node
var before = true
var after = false

func toggle():
    self.before = get_tree().is_paused()
    get_tree().set_pause(true)
    self.after = get_tree().is_paused()
",
    );

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().call_script_method(id, "toggle", &[]);

    assert_eq!(
        prop(&ml, id, "before"),
        Some(Variant::Bool(false)),
        "a fresh tree is not paused"
    );
    assert_eq!(
        prop(&ml, id, "after"),
        Some(Variant::Bool(true)),
        "set_pause must reach the tree and is_paused must observe it"
    );
    assert!(ml.tree().paused(), "…and the engine-side flag agrees");
}
