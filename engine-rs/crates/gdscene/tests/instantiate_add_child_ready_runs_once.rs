//! pat-mgixk.406 acceptance: `_ready` runs ONCE per node, and a reparent does
//! not replay it.
//!
//! Godot's `Node::_propagate_ready` notifies READY only for a node that has not
//! been readied yet, re-armable through `Node.request_ready()`
//! (`scene/main/node.cpp`). Re-entering the tree fires EXIT_TREE/ENTER_TREE but
//! **not** READY.
//!
//! # What was wrong
//!
//! `SceneTree::reparent` ends by calling `LifecycleManager::enter_tree`, whose
//! second phase notifies READY bottom-up unconditionally. So every reparent
//! replayed `_ready` for the whole moved subtree — and the editor's
//! drag-reparent (`gdeditor/src/lib.rs`) runs user callbacks through that same
//! path.
//!
//! ⚠️ The bead also described `instantiate()` parenting under `/root` so that
//! `add_child` became a reparent and ran `_ready` twice. **That half was already
//! fixed by pat-mgixk.61**: `instantiate_packed_scene` now calls
//! `instantiate_packed_scene_detached` (`scripting.rs:8653`), so the spawn path
//! below takes the plain-`add_child` branch and would pass without this change.
//! The **explicit reparent** is therefore the discriminating half of this test —
//! see the note on that assertion.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> GDScript preload(..).instantiate() -> add_child()
//!   -> MainLoop::step -> LifecycleManager::enter_tree (ENTER_TREE + READY)
//!     -> SceneTree::reparent -> enter_tree again
//! ```

use std::path::PathBuf;
use std::sync::OnceLock;

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
// `get_script()` yields `&dyn ScriptInstance`; the trait must be in scope.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// A `res://` root shared by this test binary. `gdcore`'s path roots are
/// process-global and integration tests share a process, so this is written
/// once. Copied from `runtime_instantiate_attaches_scripts_and_readies_once.rs`,
/// which drives this same spawn path and is green at HEAD.
fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("patina_mgixk406_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir is creatable");

        // The spawned child counts BOTH callbacks separately: that is what tells
        // "the lifecycle replayed" apart from "READY specifically replayed".
        std::fs::write(
            dir.join("child.gd"),
            "extends Node2D\n\
             var ready_count = 0\n\
             var enter_count = 0\n\
             \n\
             func _enter_tree():\n\
             \tself.enter_count = self.enter_count + 1\n\
             \n\
             func _ready():\n\
             \tself.ready_count = self.ready_count + 1\n",
        )
        .expect("child.gd is writable");

        std::fs::write(
            dir.join("child.tscn"),
            "[gd_scene load_steps=2 format=3]\n\n\
             [ext_resource type=\"Script\" path=\"res://child.gd\" id=\"1_c\"]\n\n\
             [node name=\"Child\" type=\"Node2D\"]\n\
             script = ExtResource(\"1_c\")\n",
        )
        .expect("child.tscn is writable");

        gdcore::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

/// The spawner does the instantiate + add_child from `_ready`, which is the
/// spelling the bead names and the one a real game uses.
const SPAWNER_GD: &str = "\
extends Node2D
func _ready():
\tvar e = preload(\"res://child.tscn\").instantiate()
\tadd_child(e)
";

fn script_prop(tree: &SceneTree, id: NodeId, name: &str) -> Variant {
    tree.get_script(id)
        .unwrap_or_else(|| panic!("node {id} must have a script to read `{name}`"))
        .get_property(name)
        .unwrap_or_else(|| panic!("script must declare `{name}`"))
}

fn node_by_name(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|&id| tree.get_node(id).map(|n| n.name()) == Some(name))
        .unwrap_or_else(|| panic!("`{name}` must exist in the tree"))
}

fn as_i64(v: &Variant, name: &str) -> i64 {
    match v {
        Variant::Int(i) => *i,
        Variant::Float(f) => *f as i64,
        other => panic!("`{name}` should be a number, got {other:?}"),
    }
}

/// `Spawner` (scripted) plus an empty `Elsewhere` to reparent into.
fn spawner_tree() -> (MainLoop, NodeId) {
    let _ = res_root();
    gdobject::class_db::register_all_classes();

    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let spawner = tree
        .add_child(root_id, Node::new("Spawner", "Node2D"))
        .expect("Spawner is addable");
    tree.add_child(root_id, Node::new("Elsewhere", "Node2D"))
        .expect("Elsewhere is addable");
    tree.attach_script(
        spawner,
        Box::new(
            GDScriptNodeInstance::from_source(SPAWNER_GD, spawner).expect("spawner.gd parses"),
        ),
    );

    let mut ml = MainLoop::new(tree);
    ml.enter_tree_with_scripts(spawner);
    (ml, spawner)
}

/// **The acceptance.**
#[test]
fn instantiate_add_child_ready_runs_once() {
    let (mut ml, _spawner) = spawner_tree();
    ml.step(DT);
    ml.step(DT);

    let child = node_by_name(ml.tree(), "Child");
    let ready =
        |ml: &MainLoop| as_i64(&script_prop(ml.tree(), child, "ready_count"), "ready_count");
    let enter =
        |ml: &MainLoop| as_i64(&script_prop(ml.tree(), child, "enter_count"), "enter_count");

    // -- the spawn itself: one _ready, one _enter_tree -----------------------
    assert_eq!(
        ready(&ml),
        1,
        "`_ready` must fire exactly once for a spawned child. 0 means the \
         script never ran at all; 2 or more means the lifecycle replayed",
    );
    assert_eq!(
        enter(&ml),
        1,
        "`_enter_tree` likewise. pat-mgixk.61 made `instantiate()` return a \
         DETACHED subtree, so `add_child` here is a plain add rather than a \
         reparent — if this is 2, that regressed and the reparent assertions \
         below would be measuring the spawn instead",
    );

    // -- the discriminating half: an explicit reparent -----------------------
    //
    // 🔑 This is what fails without the change. `SceneTree::reparent` calls
    // `LifecycleManager::enter_tree`, whose READY phase notified every node
    // unconditionally, so the moved subtree ran `_ready` again.
    //
    // ⚠️ Driven through `tree.reparent` rather than a script call because
    // GDScript has no `reparent` binding (`grep '"reparent"' interpreter.rs` is
    // empty). This is not a test-only shortcut: it is the exact entry point the
    // editor's drag-reparent uses (`gdeditor/src/lib.rs`), which is one of the
    // two places the bug was user-visible.
    let elsewhere = node_by_name(ml.tree(), "Elsewhere");
    ml.tree_mut()
        .reparent(child, elsewhere)
        .expect("reparenting the child under Elsewhere succeeds");

    assert_eq!(
        ready(&ml),
        1,
        "a reparent must NOT replay `_ready` — Godot notifies READY once per \
         node unless `request_ready()` re-arms it. 2 here is the bug: every \
         reparent, and every editor drag-reparent, re-ran user `_ready` code",
    );
    assert_eq!(
        enter(&ml),
        2,
        "...while `_enter_tree` SHOULD fire again: reparenting really does take \
         the node out of the tree and put it back, and Godot propagates \
         EXIT_TREE/ENTER_TREE for it. Asserted so the fix cannot be \
         'suppress the whole second lifecycle pass', which would also make the \
         `_ready` assertion pass",
    );

    // -- and the node really did move ---------------------------------------
    assert_eq!(
        ml.tree().get_node(child).and_then(|n| n.parent()),
        Some(elsewhere),
        "precondition for the two assertions above: the reparent actually \
         happened, so they are not passing because nothing moved",
    );
}

/// `request_ready()` re-arms `_ready`, so the next tree entry runs it again.
///
/// 🔑 The guard added for this bead skips READY for any node already readied.
/// Without this test that guard could have been written as "never ready twice",
/// which would silently break Godot's documented escape hatch — and nothing else
/// in the suite exercises it through a reparent.
#[test]
fn request_ready_rearms_ready_across_a_reparent() {
    let (mut ml, _spawner) = spawner_tree();
    ml.step(DT);
    ml.step(DT);

    let child = node_by_name(ml.tree(), "Child");
    assert_eq!(
        as_i64(&script_prop(ml.tree(), child, "ready_count"), "ready_count"),
        1,
        "precondition: one _ready from the spawn",
    );

    // Re-arm, then move it.
    // ⚠️ `request_node_ready`, not `request_ready`: the SceneTree method is
    // named for the node it targets (`scene_tree.rs:8382`); `request_ready` is
    // the method on `Node` itself.
    assert!(
        ml.tree_mut().request_node_ready(child),
        "request_node_ready must find the node",
    );
    let elsewhere = node_by_name(ml.tree(), "Elsewhere");
    ml.tree_mut()
        .reparent(child, elsewhere)
        .expect("reparent succeeds");

    assert_eq!(
        as_i64(&script_prop(ml.tree(), child, "ready_count"), "ready_count"),
        2,
        "after `request_ready()` the next tree entry MUST run `_ready` again \
         (Godot's `Node.request_ready`). 1 here means the ready-once guard is \
         unconditional and swallowed the re-arm",
    );
}
