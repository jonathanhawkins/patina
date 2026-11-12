//! pat-u3l8c.12 — `preload`/`load` return live resources: `.instantiate()`
//! builds a real subtree and `.new()` builds a real scripted instance.
//!
//! The interpreter already dispatched `PackedScene.instantiate()` and
//! `GDScript.new()` to `SceneAccess::instantiate_packed_scene` /
//! `instantiate_script`, and `gdscene` already implemented both against real
//! files — but **nothing tested the chain end to end**, and the halves did not
//! actually meet: both hooks hand-rolled `path.strip_prefix("res://")`, mapping
//! `res://bullet.tscn` to the **CWD-relative** `bullet.tscn` instead of
//! resolving it against the configured project root the way
//! `SceneTree::change_scene_to_file` does (`gdcore::resolve_path`). So
//! `preload("res://bullet.tscn").instantiate()` — how every real game spawns a
//! bullet or an enemy — only worked when the process happened to be running
//! from the project root, and returned `null` otherwise.
//!
//! These tests drive the real runtime path (`process_script_ready`, which wires
//! the `SceneTreeAccessor` the same way a live frame does) against `.tscn`/`.gd`
//! files written under a temp `res://` root, and assert against the **tree** —
//! the node really exists, with the right name/class/parent — rather than
//! against the ObjectId the script got back. A returned id proves only that a
//! number was minted; the tree proves a scene was spawned.

use std::path::PathBuf;
use std::sync::OnceLock;

use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::GDScriptNodeInstance;
// `get_script()` yields `&dyn ScriptInstance`; the trait must be in scope to
// call `get_property` on it.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// One `res://` root shared by every test in this binary.
///
/// `gdcore`'s path roots are process-global, and integration tests run as
/// threads in one process — so each test must NOT point the root somewhere
/// different or they would race. Setting it once, to one directory, keeps the
/// global write idempotent no matter the interleaving.
fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        // A per-process directory: concurrent runs of this binary (and the rest
        // of the suite) must not collide on a fixed path.
        let dir =
            std::env::temp_dir().join(format!("patina_preload_u3l8c12_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");

        // A scene to spawn: one Node2D root with a child, so we can prove the
        // whole subtree lands, not just the root node.
        std::fs::write(
            dir.join("bullet.tscn"),
            "[gd_scene format=3]\n\
             \n\
             [node name=\"Bullet\" type=\"Node2D\"]\n\
             \n\
             [node name=\"Sprite\" type=\"Sprite2D\" parent=\".\"]\n",
        )
        .expect("bullet.tscn is writable");

        // A script to instantiate. `extends Node2D` is what `.new()` must pick
        // up as the node's class.
        std::fs::write(
            dir.join("enemy.gd"),
            "extends Node2D\n\
             var hp = 7\n\
             func take_damage(n):\n\
             \tself.hp = self.hp - n\n\
             \treturn self.hp\n",
        )
        .expect("enemy.gd is writable");

        gdcore::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

/// Builds a tree whose `Main` node runs `script_src`, fires `_ready`, and hands
/// back the tree plus `Main`'s id. `process_script_ready` is the real runtime
/// entry point — it installs the `SceneTreeAccessor`, so `preload`/`load` inside
/// `_ready` reach the same host a live frame would.
fn run_ready(script_src: &str) -> (SceneTree, NodeId) {
    res_root();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main_id = tree.add_child(root, Node::new("Main", "Node")).unwrap();
    let script = GDScriptNodeInstance::from_source(script_src, main_id).expect("script parses");
    tree.attach_script(main_id, Box::new(script));
    tree.process_script_ready(main_id);
    (tree, main_id)
}

/// Reads a script variable back off `Main` after `_ready` ran.
fn prop(tree: &SceneTree, main_id: NodeId, name: &str) -> Variant {
    tree.get_script(main_id)
        .expect("Main has a script")
        .get_property(name)
        .unwrap_or(Variant::Nil)
}

/// Turns a script-held object id into a `NodeId`, failing loudly on `null` —
/// `instantiate()`/`new()` return `null` on an unloadable path, which is exactly
/// the bug this bead fixes, so a `Nil` here must not silently pass.
fn node_id_of(v: Variant, what: &str) -> NodeId {
    match v {
        Variant::ObjectId(o) => NodeId::from_object_id(o),
        Variant::Nil => panic!(
            "{what} returned null — the resource was not loaded (res:// unresolved?), \
             so no node was spawned"
        ),
        other => panic!("{what} must return an object id, got {other:?}"),
    }
}

/// The bead's acceptance: `preload(x.tscn).instantiate()` adds a real subtree.
#[test]
fn gdscript_preload_instantiate_spawns_live_scene() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var spawned = null\n\
         func _ready():\n\
         \tvar scene = preload(\"res://bullet.tscn\")\n\
         \tself.spawned = scene.instantiate()\n",
    );

    let bullet = node_id_of(prop(&tree, main_id, "spawned"), "PackedScene.instantiate()");

    // The node is really in the tree — not just an id handed back.
    let node = tree
        .get_node(bullet)
        .expect("instantiate() must add the scene root to the tree");
    assert_eq!(
        node.name(),
        "Bullet",
        "the spawned root keeps its .tscn name"
    );
    assert_eq!(
        node.class_name(),
        "Node2D",
        "the spawned root keeps its .tscn type"
    );

    // The whole subtree landed, not only the root: bullet.tscn declares a child.
    let children = node.children().to_vec();
    assert_eq!(
        children.len(),
        1,
        "instantiate() must build the scene's children, not just its root"
    );
    let child = tree.get_node(children[0]).expect("child node exists");
    assert_eq!(child.name(), "Sprite");
    assert_eq!(child.class_name(), "Sprite2D");
}

/// The bead's acceptance: `load(x.gd).new()` creates a scripted instance.
#[test]
fn gdscript_load_new_creates_scripted_instance() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var made = null\n\
         func _ready():\n\
         \tvar gd = load(\"res://enemy.gd\")\n\
         \tself.made = gd.new()\n",
    );

    let enemy = node_id_of(prop(&tree, main_id, "made"), "GDScript.new()");

    let node = tree
        .get_node(enemy)
        .expect("new() must add the scripted instance to the tree");
    assert_eq!(
        node.class_name(),
        "Node2D",
        "new() takes the node's class from the script's `extends`"
    );

    // A *scripted* instance: the script is attached and its state is live.
    let script = tree
        .get_script(enemy)
        .expect("new() must attach the loaded script, not just make a bare node");
    assert_eq!(
        script.get_property("hp"),
        Some(Variant::Int(7)),
        "the instance carries the script's initialized state"
    );
}

/// `preload` and `load` are interchangeable for this purpose (Godot differs only
/// in *when* the path is resolved), and a spawned scene can be re-parented by
/// the script — the `add_child` flow a real spawner uses.
#[test]
fn spawned_scene_can_be_reparented_by_the_script() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var spawned = null\n\
         func _ready():\n\
         \tvar scene = load(\"res://bullet.tscn\")\n\
         \tvar inst = scene.instantiate()\n\
         \tadd_child(inst)\n\
         \tself.spawned = inst\n",
    );

    let bullet = node_id_of(prop(&tree, main_id, "spawned"), "load().instantiate()");
    let parent = tree.get_node(bullet).expect("spawned node exists").parent();
    assert_eq!(
        parent,
        Some(main_id),
        "add_child(inst) must reparent the spawned scene under the caller"
    );
    assert!(
        tree.get_node(main_id)
            .expect("Main exists")
            .children()
            .contains(&bullet),
        "the spawned scene appears among its new parent's children"
    );
}

/// Two spawns from one preloaded scene are independent nodes — a spawner in a
/// loop must not keep handing back the same instance.
#[test]
fn each_instantiate_returns_a_distinct_node() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var a = null\n\
         var b = null\n\
         func _ready():\n\
         \tvar scene = preload(\"res://bullet.tscn\")\n\
         \tself.a = scene.instantiate()\n\
         \tself.b = scene.instantiate()\n",
    );

    let a = node_id_of(prop(&tree, main_id, "a"), "first instantiate()");
    let b = node_id_of(prop(&tree, main_id, "b"), "second instantiate()");
    assert_ne!(a, b, "each instantiate() must mint a separate node");
    assert!(tree.get_node(a).is_some() && tree.get_node(b).is_some());
    assert_eq!(tree.get_node(a).unwrap().name(), "Bullet");
    assert_eq!(tree.get_node(b).unwrap().name(), "Bullet");
}

/// A path that does not exist yields `null` rather than panicking — Godot's
/// `instantiate()` contract for an unloadable scene. This is the honest-failure
/// side of the res:// fix: `null` must mean "not found", which is only
/// meaningful once a *findable* path actually resolves (the tests above).
#[test]
fn instantiate_of_a_missing_scene_returns_null() {
    let (tree, main_id) = run_ready(
        "extends Node\n\
         var spawned = 1\n\
         func _ready():\n\
         \tvar scene = preload(\"res://does_not_exist.tscn\")\n\
         \tself.spawned = scene.instantiate()\n",
    );
    assert_eq!(
        prop(&tree, main_id, "spawned"),
        Variant::Nil,
        "instantiate() of a missing scene is null, not a panic"
    );
}
