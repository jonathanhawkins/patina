//! pat-mgixk.60 acceptance — Godot's `Node` hierarchy/query API reaches a
//! GDScript and actually mutates the live tree.
//!
//! Godot ground truth: `scene/main/node.cpp` `_bind_methods`,
//! `doc/classes/Node.xml`. `has_node`, `move_child`, `is_ancestor_of`,
//! `get_path_to`, `get_index` and `get_owner` are how a script rearranges its
//! own scene — reparenting a pickup onto the player, moving a card to the front
//! of a hand, asking whether a node is still under you.
//!
//! ⚠️ THE BUG WAS SILENT, which is why this test leads with `ran` and ends with
//! assertions on the TREE rather than only on script vars. Every name below is
//! registered in our ClassDB, and `node_has_method` answers from the
//! registration table alone — so the interpreter took its "the method exists"
//! branch (`interpreter.rs`, the `if responds` arm) and handed the script
//! `Variant::Nil`. A falsy value, no error, nothing logged. `get_path_to(n)`
//! returned null and a script could not even detect the problem, because the
//! thing it would ask (`has_method`) was the thing that was wrong.
//!
//! 🔑 BOTH DISPATCH SURFACES ARE EXERCISED ON PURPOSE, because they are
//! different code paths and wiring one does not wire the other:
//!   - `self.has_node("A")` / `self.get_path_to(a)` — the BARE form a script on
//!     the node itself writes, which arrives via `call_self_func_or_native`;
//!   - `$B.is_ancestor_of(a)` / `a.get_index()` — the DOT form on another node,
//!     which arrives via `call_method_on` -> `call_object_method`.
//! Both fall back to `SceneAccess::call_node_method`, which is where the arms
//! live, so one table serves both — but a test using only one spelling would
//! not have shown that.
//!
//! Entry point, per the bead's Wiring requirement: a `.tscn` instanced into a
//! real tree, a GDScript attached to its root, and one `MainLoop::step` driving
//! `_process` — the shape `analog_action_strength_reaches_scripts.rs` uses.
//! Nothing here calls a `SceneTree` method directly.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// `A` and `B` are siblings under the root and `C` sits under `B`, so after the
/// script moves `A` under `B` there is an existing sibling for `move_child` to
/// order it against. A single-child `B` would make `move_child(a, 0)` pass even
/// if it did nothing at all.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="A" type="Node2D" parent="."]

[node name="B" type="Node2D" parent="."]

[node name="C" type="Node2D" parent="B"]
"#;

/// Reads a script `var`. ⚠️ Script vars and node properties are separate
/// stores; `get_node(..).get_property(..)` would not see any of these.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// The names of a node's children, in order.
fn child_names(tree: &SceneTree, parent: NodeId) -> Vec<String> {
    tree.get_node(parent)
        .expect("parent exists")
        .children()
        .iter()
        .map(|id| {
            tree.get_node(*id)
                .map(|n| n.name().to_string())
                .unwrap_or_else(|| "<missing>".to_string())
        })
        .collect()
}

#[test]
fn script_node_hierarchy_api_mutates_live_tree() {
    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    // Sanity on the fixture itself, so a later failure cannot be the scene
    // having loaded differently than this test assumes.
    assert_eq!(
        child_names(&tree, scene_root),
        vec!["A".to_string(), "B".to_string()],
        "fixture: the root must start with A then B"
    );

    let script_src = "\
var had_a = false
var missing_is_null = false
var got_child = false
var inside = false
var ancestor_after = false
var a_index = -1
var b_index = -1
var path_matches = false
var path_to_a = null
var owner_set = false
var descendant_count = -1
var ran = false
func _process(delta):
    if self.ran:
        return
    # -- the bare (self) dispatch surface -------------------------------
    self.had_a = self.has_node(\"A\")
    self.inside = self.is_inside_tree()
    self.missing_is_null = self.get_node_or_null(\"Missing\") == null
    # Capture the node BEFORE detaching it: once it leaves the tree, `$A`
    # no longer resolves, so the handle has to be taken while it is still
    # a child.
    var a = $A
    self.owner_set = a.get_owner() != null
    self.remove_child(a)
    $B.add_child(a)
    $B.move_child(a, 0)
    # -- the dot (other-node) dispatch surface -------------------------
    self.got_child = $B.get_child(0) != null
    self.a_index = a.get_index()
    self.b_index = $B.get_index()
    self.ancestor_after = $B.is_ancestor_of(a)
    self.path_to_a = self.get_path_to(a)
    self.path_matches = self.get_path_to(a) == NodePath(\"B/A\")
    self.descendant_count = self.find_children(\"*\").size()
    self.ran = true
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_process` must run to completion — a call raising part-way through is \
         what an undispatched method looks like from here, and every assertion \
         below would then be reading its initialiser rather than a result",
    );

    // -- (a) the queries answer Godot's values, not Nil ---------------------
    for (var_name, want, why) in [
        (
            "had_a",
            true,
            "has_node(\"A\") must be true while A is still a child; Nil reads as \
             false here, which is exactly how the unwired version looked",
        ),
        (
            "missing_is_null",
            true,
            "get_node_or_null on an absent path must be null (this one already \
             worked, and is the control for the arms that did not)",
        ),
        ("inside", true, "is_inside_tree() must be true for a node in the tree"),
        (
            "got_child",
            true,
            "$B.get_child(0) must return a node after A is moved under B",
        ),
        (
            "owner_set",
            true,
            "a .tscn-authored node has its owner set to the scene root, so \
             get_owner() must not be null",
        ),
        (
            "ancestor_after",
            true,
            "$B.is_ancestor_of(a) must be true once A has been reparented under \
             B — this is the call that proves the MUTATION landed, not just that \
             the query answered",
        ),
        (
            "path_matches",
            true,
            "get_path_to(a) == NodePath(\"B/A\") is the comparison Godot's docs \
             specify; it is false if the arm returns a String that merely PRINTS \
             the same",
        ),
    ] {
        assert_eq!(
            script_var(ml.tree(), scene_root, var_name),
            Variant::Bool(want),
            "{why}"
        );
    }

    // 🔑 The type, not just the text. `get_path_to` returns a NodePath in Godot;
    // a `Variant::String` would satisfy a `str()` comparison and still break
    // every script that feeds the result to something expecting a NodePath.
    match script_var(ml.tree(), scene_root, "path_to_a") {
        Variant::NodePath(path) => assert_eq!(
            path.to_string(),
            "B/A",
            "get_path_to must spell the relative path Godot does"
        ),
        other => panic!("get_path_to must return a NodePath, got {other:?}"),
    }

    assert_eq!(
        script_var(ml.tree(), scene_root, "a_index"),
        Variant::Int(0),
        "move_child(a, 0) must put A first among B's children, so get_index() \
         reads 0; a 1 means the call was accepted and did nothing"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "b_index"),
        Variant::Int(0),
        "B is the root's only remaining child after A was removed, so its index \
         is 0 — this reads get_index() through the DOT surface, where a_index \
         read it on a detached-then-reparented node"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "descendant_count"),
        Variant::Int(3),
        "find_children(\"*\") must return B, A and C — every authored node is \
         owned by the scene root, so Godot's default owned = true keeps all \
         three; 0 would mean the owned filter rejected everything"
    );

    // -- (b) and the TREE itself is what Godot would have ------------------
    //
    // The strongest assertions in this test: script vars record what a call
    // RETURNED, while these record what the engine actually did. An arm that
    // returned plausible values without mutating anything passes everything
    // above and fails here.
    assert_eq!(
        child_names(ml.tree(), scene_root),
        vec!["B".to_string()],
        "remove_child must detach A from the root, leaving only B"
    );
    let b = ml
        .tree()
        .find_child(scene_root, "B", true)
        .expect("B still exists");
    assert_eq!(
        child_names(ml.tree(), b),
        vec!["A".to_string(), "C".to_string()],
        "A must be re-homed under B and moved ahead of C; [\"C\", \"A\"] means \
         add_child worked and move_child did not"
    );
    assert_eq!(
        ml.tree()
            .get_node(ml.tree().find_child(b, "A", false).expect("A under B"))
            .and_then(|n| n.parent()),
        Some(b),
        "A's parent pointer must agree with B's child list — a half-applied \
         reparent leaves the two disagreeing"
    );
}
