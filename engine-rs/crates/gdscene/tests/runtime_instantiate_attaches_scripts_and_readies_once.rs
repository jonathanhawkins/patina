//! pat-mgixk.61 acceptance — `instantiate()` returns a detached, script-attached
//! subtree, and `add_child` runs its lifecycle exactly once.
//!
//! Godot's `PackedScene.instantiate()` builds the node graph **outside** any
//! tree, with every node's script instance already attached
//! (`scene/resources/packed_scene.cpp SceneState::instantiate`). Nothing runs
//! until `add_child` inserts it, and `_ready` then fires once, child-first
//! (`scene/main/node.cpp _propagate_ready`).
//!
//! Patina got all three parts wrong at once:
//!
//! 1. `instantiate_packed_scene` parented the instance under `/root`
//!    immediately, so each node received ENTER_TREE/READY **on insertion** —
//!    before its children existed, making READY parent-first.
//! 2. The script's `add_child` then saw an already-parented node and
//!    `reparent`ed it, which runs `exit_tree` + `enter_tree` again — so the
//!    lifecycle fired a **second** time.
//! 3. No runtime path attached the spawned nodes' `_script_path` scripts at all.
//!    The only attach loops were at boot (patina-runner) and in the editor, so
//!    `preload("res://enemy.tscn").instantiate()` spawned nodes whose scripts
//!    never ran.
//!
//! # Why the counters are inside the spawned script
//!
//! ⚠️ A test that asserted only on the tree's SHAPE would pass against all
//! three bugs — the nodes really did exist, with the right names and parent.
//! The damage is in *when* and *whether* their code ran, which only the script's
//! own counters can see. That is also why (3) has to be fixed for this test to
//! be writable at all: with no script attached there is nothing to count, which
//! is itself the strongest statement of the bug.
//!
//! ⚠️ `ready_count == 1` is the discriminating assertion. `0` means the script
//! never attached (bug 3); `2` or more means the lifecycle replayed (bugs 1+2).
//! Asserting `>= 1` would accept the replay, and asserting "not zero" would
//! accept it too.

use std::path::PathBuf;
use std::sync::OnceLock;

use gdplatform::input::{InputEvent, InputMap, Key};
use gdscene::node::{Node, NodeId};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
// `get_script()` yields `&dyn ScriptInstance`; the trait must be in scope.
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

const DT: f64 = 1.0 / 60.0;

/// The position the spawner sets BEFORE `add_child`, which the spawned script's
/// `_enter_tree` must therefore observe.
const SPAWN_X: f64 = 5.0;

/// A `res://` root shared by this test binary.
///
/// `gdcore`'s path roots are process-global and integration tests share a
/// process, so this is written once to one directory.
fn res_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("patina_instantiate_mgixk61_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp res:// root is creatable");

        // The spawned scene. Its root carries a script via `ext_resource`, which
        // is what `.tscn` parsing records as `_script_path` — the property the
        // runtime spawn path never used to read.
        //
        // A child node is included so READY ordering is observable at all: with
        // a single node, parent-first and child-first are the same thing.
        std::fs::write(
            dir.join("enemy.tscn"),
            "[gd_scene load_steps=2 format=3]\n\
             \n\
             [ext_resource type=\"Script\" path=\"res://enemy.gd\" id=\"1_enemy\"]\n\
             \n\
             [node name=\"Enemy\" type=\"Node2D\"]\n\
             script = ExtResource(\"1_enemy\")\n\
             \n\
             [node name=\"Hitbox\" type=\"Node2D\" parent=\".\"]\n",
        )
        .expect("enemy.tscn is writable");

        // ⚠️ `enter_pos_x` is captured in `_enter_tree`, not read later by the
        // test. Reading `position` afterwards would pass even if `_enter_tree`
        // had observed the default, because the spawner's assignment would still
        // be visible by then. The whole point of a detached instance is that the
        // property is already set when the lifecycle first sees it.
        std::fs::write(
            dir.join("enemy.gd"),
            "extends Node2D\n\
             var enter_count = 0\n\
             var ready_count = 0\n\
             var process_ticks = 0\n\
             var enter_pos_x = -1.0\n\
             \n\
             func _enter_tree():\n\
             \tself.enter_count = self.enter_count + 1\n\
             \tself.enter_pos_x = self.position.x\n\
             \n\
             func _ready():\n\
             \tself.ready_count = self.ready_count + 1\n\
             \n\
             func _process(delta):\n\
             \tself.process_ticks = self.process_ticks + 1\n\
             \tself.position = Vector2(self.position.x + 1.0, self.position.y)\n",
        )
        .expect("enemy.gd is writable");

        gdcore::set_path_roots(Some(dir.clone()), None);
        dir
    })
}

/// The spawner: on a SPACE press it instantiates the scene, sets a property on
/// the detached instance, and adds it as its own child — all inside one
/// `_process`, so the whole chain happens in a single `MainLoop::step`.
const SPAWNER_GD: &str = "extends Node2D\n\
     var spawned = false\n\
     var spawn_failed = false\n\
     \n\
     func _process(delta):\n\
     \tif self.spawned:\n\
     \t\treturn\n\
     \tif not Input.is_key_pressed(KEY_SPACE):\n\
     \t\treturn\n\
     \tvar e = preload(\"res://enemy.tscn\").instantiate()\n\
     \tif e == null:\n\
     \t\tself.spawn_failed = true\n\
     \t\treturn\n\
     \te.position = Vector2(5, 5)\n\
     \tadd_child(e)\n\
     \tself.spawned = true\n";

fn key_event(key: Key, pressed: bool) -> InputEvent {
    InputEvent::Key {
        key,
        pressed,
        shift: false,
        ctrl: false,
        alt: false,
    }
}

fn spawner_scene() -> (MainLoop, NodeId) {
    res_root();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let spawner = tree
        .add_child(root, Node::new("Spawner", "Node2D"))
        .unwrap();
    tree.attach_script(
        spawner,
        Box::new(GDScriptNodeInstance::from_source(SPAWNER_GD, spawner).expect("spawner parses")),
    );
    let mut ml = MainLoop::new(tree);
    ml.set_input_map(InputMap::new());
    (ml, spawner)
}

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

fn as_f64(v: &Variant, name: &str) -> f64 {
    match v {
        Variant::Float(f) => *f,
        Variant::Int(i) => *i as f64,
        other => panic!("`{name}` should be a number, got {other:?}"),
    }
}

/// **The acceptance test.**
#[test]
fn runtime_instantiate_attaches_scripts_and_readies_once() {
    let (mut ml, spawner) = spawner_scene();

    // -- control: nothing spawns without the key ----------------------------
    //
    // Without this, an implementation that spawned on every frame regardless of
    // input would satisfy everything below.
    ml.step(DT);
    assert_eq!(
        script_prop(ml.tree(), spawner, "spawned"),
        Variant::Bool(false),
        "nothing may spawn before the key is pressed, or the assertions below \
         cannot tell a key-driven spawn from an unconditional one"
    );

    // -- the spawn frame ----------------------------------------------------
    ml.push_event(key_event(Key::Space, true));
    ml.step(DT);

    assert_eq!(
        script_prop(ml.tree(), spawner, "spawn_failed"),
        Variant::Bool(false),
        "`preload(\"res://enemy.tscn\").instantiate()` must not return null"
    );
    assert_eq!(
        script_prop(ml.tree(), spawner, "spawned"),
        Variant::Bool(true),
        "the SPACE press must have driven the spawn inside one MainLoop::step"
    );

    let enemy = node_by_name(ml.tree(), "Enemy");

    // -- (3) the spawned node's own script is attached and live -------------
    //
    // Asserted first because it gates everything else: with no script there is
    // nothing to count, and that was the original state of this path.
    assert!(
        ml.tree().get_script(enemy).is_some(),
        "the spawned scene's root must have its `_script_path` script attached. \
         None attached is the core pat-mgixk.61 bug: the only attach loops were \
         at boot and in the editor, so a spawned enemy ran as if scriptless — no \
         _ready, no _process, no signal handlers."
    );

    // -- (1)+(2) the lifecycle ran EXACTLY once -----------------------------
    assert_eq!(
        as_i64(&script_prop(ml.tree(), enemy, "ready_count"), "ready_count"),
        1,
        "`_ready` must fire exactly once. 0 means the script never ran; 2 or \
         more means the lifecycle replayed — the instance was parented under \
         /root on creation and then `add_child` reparented it, running \
         exit_tree/enter_tree a second time."
    );
    assert_eq!(
        as_i64(&script_prop(ml.tree(), enemy, "enter_count"), "enter_count"),
        1,
        "`_enter_tree` must fire exactly once, for the same reason as \
         `_ready` above"
    );

    // -- the instance was DETACHED when the spawner set its property --------
    //
    // ⚠️ This is what "detached" buys and why it is worth the change: the
    // property was set before the node was in a tree, so its own `_enter_tree`
    // already sees it. If the instance had been live at creation, `_enter_tree`
    // would have run before the spawner's assignment and seen the default.
    assert!(
        (as_f64(&script_prop(ml.tree(), enemy, "enter_pos_x"), "enter_pos_x") - SPAWN_X).abs()
            < 1e-6,
        "`_enter_tree` must observe the position the spawner set BEFORE \
         add_child ({SPAWN_X}); got {:?}. The declared default is -1.0, and 0.0 \
         would mean _enter_tree ran before the assignment — i.e. the instance \
         was already in the tree when it was created.",
        script_prop(ml.tree(), enemy, "enter_pos_x")
    );

    // -- it is parented where the script put it, never under /root ----------
    let parent = ml
        .tree()
        .get_node(enemy)
        .and_then(|n| n.parent())
        .expect("the spawned enemy must have a parent after add_child");
    assert_eq!(
        parent, spawner,
        "the enemy must be a child of the node whose script called add_child, \
         not of /root. Parenting under /root first is what forced the reparent \
         that replayed the lifecycle."
    );
    let root = ml.tree().root_id();
    let root_children = ml
        .tree()
        .get_node(root)
        .map(|n| n.children().to_vec())
        .unwrap_or_default();
    assert!(
        !root_children.contains(&enemy),
        "the enemy must never appear as a child of /root"
    );

    // -- and the subtree came with it ---------------------------------------
    let hitbox = node_by_name(ml.tree(), "Hitbox");
    assert_eq!(
        ml.tree().get_node(hitbox).and_then(|n| n.parent()),
        Some(enemy),
        "the spawned scene's child node must have moved with its root; a \
         detached instance has to carry its whole subtree through add_child"
    );

    // -- the spawned script keeps running on later frames -------------------
    //
    // A script can be attached and still never be driven. This is the half that
    // proves the spawned node joined the live frame loop rather than being
    // inserted as inert data.
    let ticks_before = as_i64(
        &script_prop(ml.tree(), enemy, "process_ticks"),
        "process_ticks",
    );
    ml.step(DT);
    ml.step(DT);
    let ticks_after = as_i64(
        &script_prop(ml.tree(), enemy, "process_ticks"),
        "process_ticks",
    );
    assert!(
        ticks_after >= ticks_before + 2,
        "the spawned node's `_process` must run on later frames: ticks went \
         {ticks_before} -> {ticks_after} across two steps. No advance means the \
         node is in the tree but not being driven."
    );

    // And the frames had a visible effect on the node, not just on a counter: a
    // counter alone would still advance if `_process` ran but its writes went
    // nowhere.
    let moved_x = match ml
        .tree()
        .get_node(enemy)
        .map(|n| n.get_property("position"))
    {
        Some(Variant::Vector2(v)) => v.x as f64,
        other => panic!("the enemy must carry a Vector2 `position`, got {other:?}"),
    };
    assert!(
        moved_x > SPAWN_X,
        "`_process` must actually move the node: position.x is {moved_x}, but it \
         spawned at {SPAWN_X} and each frame adds 1.0. Unchanged means the \
         script ran and its property writes did not reach the node."
    );
}
