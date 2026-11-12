//! A method that re-enters its own script — it emits a signal it handles
//! itself, or calls a node that calls back — must see, and keep, the members
//! that nested call changed. Godot's members are live.
//!
//! Found while making every test touching the game's paths pass: the nested
//! frame's scopes are stacked on the caller's, so the "is this bare name a
//! shadowing local?" check saw the nested frame's own member seed as a SECOND
//! definition and wrote `n = n + 1` into a throwaway local. Handlers invoked
//! that way changed nothing, and the caller read stale values after the call.

use gdscene::node::Node;
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop, NodeId, SceneTree};
use gdvariant::Variant;

fn boot(scripts: &[(&str, &str, Option<&str>)]) -> (MainLoop, Vec<NodeId>) {
    gdobject::class_db::register_all_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut ids = Vec::new();
    for (name, src, parent) in scripts {
        let parent_id = match parent {
            Some(p) => tree.get_node_relative(ids[0], p).unwrap_or(ids[0]),
            None => root,
        };
        let id = tree
            .add_child(parent_id, Node::new(*name, "Node2D"))
            .unwrap();
        tree.attach_script(
            id,
            Box::new(GDScriptNodeInstance::from_source(src, id).unwrap()),
        );
        ids.push(id);
    }
    LifecycleManager::enter_tree(&mut tree, ids[0]);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);
    (ml, ids)
}

fn var(ml: &MainLoop, id: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(id)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

const SELF_EMIT: &str = "\
extends Node2D
signal hit
var hp = 10
var seen_after = -1
var ran = false
func _ready():
    hit.connect(_on_hit)
func _process(d):
    if ran:
        return
    ran = true
    hit.emit()
    self.seen_after = hp
func _on_hit():
    hp = hp - 3
";

#[test]
fn a_handler_for_the_scripts_own_signal_keeps_its_writes_and_the_emitter_sees_them() {
    let (ml, ids) = boot(&[("P", SELF_EMIT, None)]);
    assert_eq!(
        var(&ml, ids[0], "hp"),
        Variant::Int(7),
        "the handler's write is kept"
    );
    assert_eq!(
        var(&ml, ids[0], "seen_after"),
        Variant::Int(7),
        "the emitting method reads the live member after the handler ran"
    );
}

const LOCAL_SHADOW: &str = "\
extends Node2D
var n = 0
var local_after = -1
var ran = false
func _process(d):
    if ran:
        return
    ran = true
    var n = 100
    bump()
    self.local_after = n
func bump():
    n = n + 1
";

#[test]
fn a_callers_local_does_not_capture_a_callees_member_write() {
    let (ml, ids) = boot(&[("P", LOCAL_SHADOW, None)]);
    assert_eq!(
        var(&ml, ids[0], "n"),
        Variant::Int(1),
        "bump() wrote the MEMBER n"
    );
    assert_eq!(
        var(&ml, ids[0], "local_after"),
        Variant::Int(100),
        "the caller's own local is untouched by the callee"
    );
}

const PLAYER: &str = "\
extends Node2D
var died_count = 0
var ran = false
func _ready():
    $Enemy.died.connect(_on_died)
func _process(d):
    if ran:
        return
    ran = true
    $Enemy.take_damage(10)
func _on_died():
    died_count = died_count + 1
";

const ENEMY: &str = "\
extends Node2D
signal died
var hp = 10
func take_damage(n):
    hp = hp - n
    if hp <= 0:
        died.emit()
";

#[test]
fn a_callback_into_the_caller_from_a_node_it_called_is_kept() {
    let (ml, ids) = boot(&[("Player", PLAYER, None), ("Enemy", ENEMY, Some("."))]);
    assert_eq!(var(&ml, ids[1], "hp"), Variant::Int(0));
    assert_eq!(
        var(&ml, ids[0], "died_count"),
        Variant::Int(1),
        "Enemy's `died`, emitted while Player's _process is running, reaches Player and sticks"
    );
}
