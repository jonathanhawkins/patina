//! pat-0q2kx.47 acceptance: a script drives scene flow through `get_tree()`.
//!
//! `SceneTree`'s control surface — `change_scene_to_file`, `quit`, `set_pause`,
//! the group broadcasts — was fully implemented and unit-tested engine-side, and
//! completely unreachable from a script: the `SceneAccess` trait exposed none of
//! it, so `get_tree().quit()` died on `UndefinedFunction`. `quit` in particular
//! was doubly inert — nothing set it from a script, and no host loop ever read
//! `should_quit`, so even the engine-side call did nothing observable.
//!
//! Every test here goes through a real parsed `.gd` attached to a real node and
//! driven by `MainLoop::step`, so nothing is proved by calling the engine API
//! directly — the point of the bead is the bridge, not the implementation.

use gdcore::set_path_roots;
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DELTA: f64 = 1.0 / 60.0;

/// Parses `source` and attaches it to `id`, as the runner and editor do.
fn attach(tree: &mut SceneTree, id: NodeId, source: &str) {
    let instance = GDScriptNodeInstance::from_source(source, id).expect("script must parse");
    tree.attach_script(id, Box::new(instance));
}

/// Registers `id` in `group` the way a loaded scene does.
///
/// `Node::add_to_group` alone is not enough: `SceneTree` maintains its own group
/// index, `nodes_in_group_ordered` (which every broadcast goes through) reads only
/// that index, and `add_child` does not sync a node's pre-existing groups into it.
/// A node grouped only on itself reports `is_in_group` true while being invisible
/// to `call_group` — so the tree-level call is the one that counts.
fn register_group(tree: &mut SceneTree, id: NodeId, group: &str) {
    tree.add_to_group(id, group)
        .expect("node must be in the tree before it can join a group");
}

fn script_var(tree: &SceneTree, id: NodeId, name: &str) -> Option<Variant> {
    tree.get_script(id)
        .expect("node must have a script attached")
        .get_property(name)
}

fn node_names(tree: &SceneTree) -> Vec<String> {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .filter_map(|id| tree.get_node(id).map(|n| n.name().to_string()))
        .collect()
}

/// **The acceptance test.** A script calling `get_tree().change_scene_to_file`
/// in `_process` swaps the running scene at the frame boundary.
///
/// The swap is deferred exactly as Godot defers it, which is what makes the call
/// safe: the frame that asks finishes in the old scene, so the calling script's
/// own node is not freed underneath it mid-callback.
#[test]
fn scene_tree_script_bridge_scene_management() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("b.tscn"),
        "[gd_scene format=3]\n\n[node name=\"SceneB\" type=\"Node2D\"]\n",
    )
    .expect("write b.tscn");
    // `res://` resolution is process-global; nextest runs each test in its own
    // process, so this is the only test here that touches it.
    set_path_roots(Some(dir.path().to_path_buf()), None);

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    attach(
        &mut tree,
        a,
        "extends Node\n\
         \n\
         var switched = false\n\
         \n\
         func _process(_delta):\n\
         \tif not switched:\n\
         \t\tswitched = true\n\
         \t\tget_tree().change_scene_to_file(\"res://b.tscn\")\n",
    );

    let mut ml = MainLoop::new(tree);
    assert!(
        !ml.tree().has_pending_scene_change(),
        "nothing queued before the first frame",
    );

    ml.step(DELTA);

    let names = node_names(ml.tree());
    assert!(
        names.iter().any(|n| n == "SceneB"),
        "the requested scene must be live after the frame boundary: {names:?}",
    );
    assert!(
        !names.iter().any(|n| n == "A"),
        "the previous scene must be gone: {names:?}",
    );
    assert!(
        !ml.tree().has_pending_scene_change(),
        "the queued change must be consumed, not re-applied every frame",
    );
}

/// `get_tree().quit(3)` records the request and the exit code the host loop
/// exits with.
///
/// Both halves were dead: no script could call `quit`, and `should_quit` had no
/// consumer. The runner now stops stepping and exits with this code; the editor
/// tears the Play session down.
#[test]
fn script_quit_records_the_exit_code() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    attach(
        &mut tree,
        a,
        "extends Node\n\nfunc _process(_delta):\n\tget_tree().quit(3)\n",
    );

    let mut ml = MainLoop::new(tree);
    assert!(
        !ml.tree().should_quit(),
        "no quit requested before stepping"
    );
    assert_eq!(ml.tree().exit_code(), 0, "default exit code is 0");

    ml.step(DELTA);

    assert!(
        ml.tree().should_quit(),
        "get_tree().quit() must reach SceneTree::quit",
    );
    assert_eq!(
        ml.tree().exit_code(),
        3,
        "the script's exit code must survive to the host loop",
    );
}

/// `get_tree().quit()` with no argument uses Godot's default exit code of 0 —
/// the argument is optional, and requiring it would break the common call.
#[test]
fn script_quit_defaults_to_zero() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let a = tree.add_child(root, Node::new("A", "Node")).unwrap();
    attach(
        &mut tree,
        a,
        "extends Node\n\nfunc _process(_delta):\n\tget_tree().quit()\n",
    );

    let mut ml = MainLoop::new(tree);
    ml.step(DELTA);

    assert!(
        ml.tree().should_quit(),
        "a bare quit() must still request it"
    );
    assert_eq!(ml.tree().exit_code(), 0);
}

/// `get_tree().call_group("enemies", "hit")` runs `hit()` on the grouped nodes'
/// **scripts**, not merely on nodes that happen to be in the group.
#[test]
fn script_call_group_invokes_grouped_scripts() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let boss = tree.add_child(root, Node::new("Boss", "Node")).unwrap();

    let mut enemies = Vec::new();
    for name in ["E1", "E2"] {
        let id = tree.add_child(root, Node::new(name, "Node")).unwrap();
        // Membership must be registered through the TREE. `SceneTree` keeps its
        // own group index and every group query reads that index, so a node that
        // carries the group only on itself — what `Node::add_to_group` does — is
        // invisible to `call_group`, and `add_child` never syncs the two.
        register_group(&mut tree, id, "enemies");
        attach(
            &mut tree,
            id,
            "extends Node\n\nvar hits = 0\n\nfunc hit():\n\thits += 1\n",
        );
        enemies.push(id);
    }

    // The broadcaster is deliberately outside the group: a script that is
    // currently running is checked out of the tree's script arena, so a group
    // call whose membership included the caller could not re-enter it.
    attach(
        &mut tree,
        boss,
        "extends Node\n\
         \n\
         var fired = false\n\
         \n\
         func _process(_delta):\n\
         \tif not fired:\n\
         \t\tfired = true\n\
         \t\tget_tree().call_group(\"enemies\", \"hit\")\n",
    );

    let mut ml = MainLoop::new(tree);
    ml.step(DELTA);

    for id in enemies {
        assert_eq!(
            script_var(ml.tree(), id, "hits"),
            Some(Variant::Int(1)),
            "each grouped node's script must have run hit() exactly once",
        );
    }
    assert_eq!(
        script_var(ml.tree(), boss, "fired"),
        Some(Variant::Bool(true)),
        "the broadcaster's own frame must have completed",
    );
}

/// `set_group` writes a property across a group, and `notify_group` delivers a
/// notification code to every member — the non-call halves of the broadcast API.
#[test]
fn script_set_group_and_notify_group_reach_every_member() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let boss = tree.add_child(root, Node::new("Boss", "Node")).unwrap();

    let mut enemies = Vec::new();
    for name in ["E1", "E2"] {
        let id = tree.add_child(root, Node::new(name, "Node")).unwrap();
        register_group(&mut tree, id, "enemies");
        enemies.push(id);
    }

    attach(
        &mut tree,
        boss,
        "extends Node\n\
         \n\
         var fired = false\n\
         \n\
         func _process(_delta):\n\
         \tif not fired:\n\
         \t\tfired = true\n\
         \t\tget_tree().set_group(\"enemies\", \"alerted\", true)\n\
         \t\tget_tree().notify_group(\"enemies\", 42)\n",
    );

    let mut ml = MainLoop::new(tree);
    ml.step(DELTA);

    for id in enemies {
        let node = ml.tree().get_node(id).expect("member still live");
        assert_eq!(
            node.get_property("alerted"),
            Variant::Bool(true),
            "set_group must write the property on every member",
        );
        assert!(
            node.notification_log().iter().any(|n| n.code() == 42),
            "notify_group must deliver the code to every member, log was {:?}",
            node.notification_log(),
        );
    }
}

/// The read side: a script can interrogate the tree it is running in and pause
/// it. `is_paused` reflects the pause it just requested, so the setter and the
/// getter address the same state rather than two copies of it.
#[test]
fn script_reads_tree_state_and_pauses() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let enemy = tree.add_child(root, Node::new("E1", "Node")).unwrap();
    register_group(&mut tree, enemy, "enemies");
    let probe = tree.add_child(root, Node::new("Probe", "Node")).unwrap();
    attach(
        &mut tree,
        probe,
        "extends Node\n\
         \n\
         var saw_group = false\n\
         var missing_group = true\n\
         var has_first = false\n\
         var node_count = 0\n\
         var root_ok = false\n\
         var paused_before = true\n\
         var paused_after = false\n\
         \n\
         func _process(_delta):\n\
         \tsaw_group = get_tree().has_group(\"enemies\")\n\
         \tmissing_group = get_tree().has_group(\"nobody\")\n\
         \thas_first = get_tree().get_first_node_in_group(\"enemies\") != null\n\
         \tnode_count = get_tree().get_node_count()\n\
         \troot_ok = get_tree().get_root() != null\n\
         \tpaused_before = get_tree().is_paused()\n\
         \tget_tree().set_pause(true)\n\
         \tpaused_after = get_tree().is_paused()\n",
    );

    let mut ml = MainLoop::new(tree);
    ml.step(DELTA);

    assert_eq!(
        script_var(ml.tree(), probe, "saw_group"),
        Some(Variant::Bool(true)),
        "has_group must see a populated group",
    );
    assert_eq!(
        script_var(ml.tree(), probe, "missing_group"),
        Some(Variant::Bool(false)),
        "has_group must be false for a group with no members — a blanket true \
         would make the previous assertion vacuous",
    );
    assert_eq!(
        script_var(ml.tree(), probe, "has_first"),
        Some(Variant::Bool(true)),
        "get_first_node_in_group must return a live handle",
    );
    // root + E1 + Probe.
    assert_eq!(
        script_var(ml.tree(), probe, "node_count"),
        Some(Variant::Int(3)),
        "get_node_count must report the real tree size",
    );
    assert_eq!(
        script_var(ml.tree(), probe, "root_ok"),
        Some(Variant::Bool(true)),
        "get_root must hand back a usable object handle",
    );
    assert_eq!(
        script_var(ml.tree(), probe, "paused_before"),
        Some(Variant::Bool(false)),
        "the tree starts unpaused",
    );
    assert_eq!(
        script_var(ml.tree(), probe, "paused_after"),
        Some(Variant::Bool(true)),
        "is_paused must observe the set_pause the same script just made",
    );
    assert!(
        ml.tree().paused(),
        "the pause must land on the engine's own state, not a script-side copy",
    );
}
