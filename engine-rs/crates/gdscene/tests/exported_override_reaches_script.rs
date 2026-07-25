//! pat-0q2kx.17 — a `.tscn` exported-property override reaches the script instance.
//!
//! Godot applies a scene's stored node properties through `Object::set` while
//! instantiating (packed_scene.cpp `SceneState::instantiate`), which routes each
//! exported value into the script instance and runs that property's setter
//! (gdscript.cpp `GDScriptInstance::set`). Patina attached the parsed script but
//! never copied the node's `.tscn` values into it, so `@export var speed := 200.0`
//! with a scene override of `speed = 300.0` ran with **200** and any declared
//! setter never fired.
//!
//! The fix lives in `SceneTree::attach_script`, which is the single funnel the
//! runner's boot path, the runner's main path and the editor's Play path all use —
//! so this test drives that funnel in the same order the runner does (instance the
//! scene, then attach), and the assertions hold for all three by construction.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// `Player` carries the override; `Watcher` reads it back across nodes.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Player" type="Node2D" parent="."]
speed = 300.0

[node name="Watcher" type="Node2D" parent="."]
"#;

/// A scene with no `speed =` line, to prove the override path does not invent one.
const SCENE_NO_OVERRIDE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Player" type="Node2D" parent="."]
"#;

/// `speed` declares a setter, so the override must arrive *through* it.
/// (`self.speed = value` inside the setter writes the backing store rather than
/// recursing — the interpreter guards that case.)
const PLAYER_SCRIPT: &str = "\
extends Node2D
var setter_calls = 0
var speed_seen = 0.0
@export var speed = 200.0: set = set_speed
func set_speed(value):
    self.setter_calls = self.setter_calls + 1
    self.speed = value
func _ready():
    self.speed_seen = self.speed
";

const WATCHER_SCRIPT: &str = "\
extends Node2D
var seen = 0.0
func _ready():
    self.seen = get_node(\"../Player\").speed
";

fn script_prop(tree: &SceneTree, node: NodeId, prop: &str) -> Variant {
    tree.get_script(node)
        .expect("node has a script")
        .get_property(prop)
        .unwrap_or(Variant::Nil)
}

/// Builds the scene and attaches both scripts in the runner's order.
fn boot(scene_src: &str, with_watcher: bool) -> (SceneTree, NodeId, NodeId, NodeId) {
    let scene = PackedScene::from_tscn(scene_src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let player = tree
        .get_node_or_null(scene_root, "Player")
        .expect("Player node");
    let watcher = if with_watcher {
        tree.get_node_or_null(scene_root, "Watcher")
            .expect("Watcher node")
    } else {
        player
    };

    let player_script =
        GDScriptNodeInstance::from_source(PLAYER_SCRIPT, player).expect("player script parses");
    tree.attach_script(player, Box::new(player_script));
    if with_watcher {
        let watcher_script =
            GDScriptNodeInstance::from_source(WATCHER_SCRIPT, watcher).expect("watcher parses");
        tree.attach_script(watcher, Box::new(watcher_script));
    }
    (tree, scene_root, player, watcher)
}

#[test]
fn exported_tscn_override_reaches_script() {
    let (mut tree, scene_root, player, watcher) = boot(SCENE, true);

    // The override is on the node bag from the `.tscn` — that part already worked.
    assert_eq!(
        tree.get_node(player).unwrap().get_property("speed"),
        Variant::Float(300.0),
        "the .tscn value should be on the node's property bag"
    );

    // …and attaching the script must have carried it into the instance, through
    // the setter, before any lifecycle callback runs.
    assert_eq!(
        script_prop(&tree, player, "speed"),
        Variant::Float(300.0),
        "script instance still holds its default instead of the scene override"
    );
    assert_eq!(
        script_prop(&tree, player, "setter_calls"),
        Variant::Int(1),
        "the declared setter must run exactly once, with the override"
    );

    // `_ready` observes the override, not the default.
    LifecycleManager::enter_tree(&mut tree, scene_root);
    assert_eq!(
        script_prop(&tree, player, "speed_seen"),
        Variant::Float(300.0),
        "_ready saw the pre-override default"
    );

    // A sibling script reading it across nodes sees the same value.
    assert_eq!(
        script_prop(&tree, watcher, "seen"),
        Variant::Float(300.0),
        "get_node(\"../Player\").speed must report the override"
    );

    // Running frames must not re-apply the override or re-fire the setter.
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);
    ml.step(1.0 / 60.0);
    assert_eq!(
        script_prop(ml.tree(), player, "setter_calls"),
        Variant::Int(1),
        "the setter fired again after attach"
    );
    assert_eq!(
        script_prop(ml.tree(), player, "speed"),
        Variant::Float(300.0),
        "the override was clobbered by a later sync"
    );
}

/// The mirror case: with no override in the scene, the script's own default must
/// survive and the setter must not fire — otherwise "apply overrides" would have
/// become "overwrite everything with whatever the node bag happens to hold".
#[test]
fn script_default_survives_when_scene_has_no_override() {
    let (tree, _root, player, _w) = boot(SCENE_NO_OVERRIDE, false);

    assert_eq!(
        script_prop(&tree, player, "speed"),
        Variant::Float(200.0),
        "script default was replaced even though the scene overrode nothing"
    );
    assert_eq!(
        script_prop(&tree, player, "setter_calls"),
        Variant::Int(0),
        "setter fired without an override to apply"
    );
}
