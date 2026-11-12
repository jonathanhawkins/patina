//! pat-s3rrv.1 — `AnimationNodeStateMachinePlayback` reachable from GDScript.
//!
//! Godot ground truth (`doc/classes/AnimationNodeStateMachinePlayback.xml`,
//! `scene/animation/animation_node_state_machine.cpp`): a game steers a state
//! machine through the playback object, never by poking transition conditions:
//!
//! ```gdscript
//! var p = $AnimationTree.get("parameters/playback")
//! p.travel("jump")
//! ```
//!
//! Patina had the BFS travel algorithm implemented and unit-tested inside
//! `animation_tree.rs`, but with **zero** runtime callers: `get("parameters/playback")`
//! answered `Nil`, so nothing a script could hold reached it. This test pins the
//! whole path end to end — script read → handle → `travel` → `MainLoop::step`
//! crossfades along the path → the driven node lands on the target clip's pose.

use gdscene::animation_tree::AnimationNode;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdscript_interop::bindings::ScriptInstance;
use gdvariant::Variant;

/// A `.tscn` in Godot's own layout: an `AnimationPlayer` holding three clips and
/// an `AnimationTree` whose `tree_root` is a three-state machine chained
/// `idle -> run -> jump`. There is **no** transition from `idle` to `jump`, so
/// reaching `jump` requires walking the graph — which is exactly what `travel`
/// does and what a condition flip cannot do.
///
/// Each clip holds `y` at a distinct constant (idle 100, run 50, jump 0) so the
/// arrival is observable as a value, and ramps `x` so time-advance is visible
/// separately from blending. The two hops declare a 0.2s crossfade each.
const TRAVEL_SCENE: &str = r#"[gd_scene load_steps=12 format=3]

[sub_resource type="Animation" id="Animation_idle"]
resource_name = "idle"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:x")
tracks/0/interp = 1
tracks/0/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [0.0, 10.0]}
tracks/1/type = "value"
tracks/1/enabled = true
tracks/1/path = NodePath("Sprite:y")
tracks/1/interp = 1
tracks/1/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [100.0, 100.0]}

[sub_resource type="Animation" id="Animation_run"]
resource_name = "run"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:x")
tracks/0/interp = 1
tracks/0/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [0.0, 100.0]}
tracks/1/type = "value"
tracks/1/enabled = true
tracks/1/path = NodePath("Sprite:y")
tracks/1/interp = 1
tracks/1/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [50.0, 50.0]}

[sub_resource type="Animation" id="Animation_jump"]
resource_name = "jump"
length = 2.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:x")
tracks/0/interp = 1
tracks/0/keys = {"times": PackedFloat32Array(0, 2), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [0.0, 500.0]}
tracks/1/type = "value"
tracks/1/enabled = true
tracks/1/path = NodePath("Sprite:y")
tracks/1/interp = 1
tracks/1/keys = {"times": PackedFloat32Array(0, 2), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [0.0, 0.0]}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {"idle": SubResource("Animation_idle"), "run": SubResource("Animation_run"), "jump": SubResource("Animation_jump")}

[sub_resource type="AnimationNodeAnimation" id="Anim_idle"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="Anim_run"]
animation = &"run"

[sub_resource type="AnimationNodeAnimation" id="Anim_jump"]
animation = &"jump"

[sub_resource type="AnimationNodeStateMachineTransition" id="Trans_start"]
xfade_time = 0.0

[sub_resource type="AnimationNodeStateMachineTransition" id="Trans_idle_run"]
xfade_time = 0.2

[sub_resource type="AnimationNodeStateMachineTransition" id="Trans_run_jump"]
xfade_time = 0.2

[sub_resource type="AnimationNodeStateMachine" id="StateMachine_main"]
states/idle/node = SubResource("Anim_idle")
states/idle/position = Vector2(300, 100)
states/run/node = SubResource("Anim_run")
states/run/position = Vector2(500, 100)
states/jump/node = SubResource("Anim_jump")
states/jump/position = Vector2(700, 100)
transitions = ["Start", "idle", SubResource("Trans_start"), "idle", "run", SubResource("Trans_idle_run"), "run", "jump", SubResource("Trans_run_jump")]

[node name="Character" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
x = 0.0
y = 0.0

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("StateMachine_main")
anim_player = NodePath("../AnimationPlayer")
active = true
"#;

/// Reads a numeric node property.
fn num_prop(tree: &SceneTree, node: NodeId, prop: &str) -> f64 {
    match tree.get_node(node).expect("node exists").get_property(prop) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{prop} is not numeric: {other:?}"),
    }
}

/// Reads a script `var` off the attached script instance. Node properties and
/// script vars are separate stores — `get_node(..).get_property(..)` would
/// answer `Nil` here and fail the assertion for the wrong reason.
fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

/// The state machine's current state, read straight off the attached graph —
/// the engine-side check that the script's report is not self-consistent
/// fiction.
fn current_state(tree: &SceneTree, anim_tree_node: NodeId) -> String {
    match &tree
        .get_animation_tree(anim_tree_node)
        .expect("AnimationTree attached at scene load")
        .root
    {
        Some(AnimationNode::StateMachine(sm)) => sm.current.clone(),
        Some(AnimationNode::Named { inner, .. }) => match inner.as_ref() {
            AnimationNode::StateMachine(sm) => sm.current.clone(),
            other => panic!("tree_root is not a state machine: {other:?}"),
        },
        other => panic!("tree_root is not a state machine: {other:?}"),
    }
}

/// The acceptance test for pat-s3rrv.1.
///
/// A script grabs `parameters/playback` in `_ready` and calls `travel("jump")`.
/// Two hops away, behind two 0.2s crossfades: the machine must walk the path
/// from `MainLoop::step` alone, and the playback object must report the path it
/// is walking, the node it is on, and the fade it is in — the whole surface a
/// state-machine game reads back.
#[test]
fn animation_tree_state_machine_playback_travel_from_script() {
    let scene = PackedScene::from_tscn(TRAVEL_SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let sprite = tree
        .get_node_or_null(scene_root, "Sprite")
        .expect("Sprite node");
    let anim_tree_node = tree
        .get_node_or_null(scene_root, "AnimationTree")
        .expect("AnimationTree node");

    assert_eq!(
        current_state(&tree, anim_tree_node),
        "idle",
        "the Start transition selects the initial state"
    );

    // Exactly the Godot idiom. `_ready` records what the playback object reports
    // *before* any frame has run, so the queued path is observable at its
    // longest; `_process` records what it reports as the walk proceeds.
    // (Array literals stay off multi-line form — the parser rejects those.)
    let script_src = "\
extends Node2D
var start_node = \"?\"
var path_len = -1
var hop0 = \"?\"
var hop1 = \"?\"
var idle_len = -1.0
var travelled = false
var cur = \"?\"
var playing = false
var fading_from = \"?\"
func _ready():
    var p = get_node(\"AnimationTree\").get(\"parameters/playback\")
    self.start_node = p.get_current_node()
    self.idle_len = p.get_current_length()
    p.travel(\"jump\")
    var path = p.get_travel_path()
    self.path_len = path.size()
    self.hop0 = path[0]
    self.hop1 = path[1]
    self.travelled = true
func _process(delta):
    var p = get_node(\"AnimationTree\").get(\"parameters/playback\")
    self.cur = p.get_current_node()
    self.playing = p.is_playing()
    self.fading_from = p.get_fading_from_node()
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    // The scene's own ready pass ran before the script was attached, so fire it
    // explicitly — this is the frame the game's `travel()` happens on.
    ml.tree_mut().process_script_ready(scene_root);

    // -- what `_ready` saw: the playback object exists and answers -------------
    assert_eq!(
        script_var(ml.tree(), scene_root, "travelled"),
        Variant::Bool(true),
        "`_ready` must run to completion — an earlier statement raised"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "start_node"),
        Variant::String("idle".to_string()),
        "get_current_node() reports the machine's initial state"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "idle_len"),
        Variant::Float(1.0),
        "get_current_length() reports the idle clip's length from the library"
    );

    // -- the queued path: two hops, in order, because idle -> jump is not an edge
    assert_eq!(
        script_var(ml.tree(), scene_root, "path_len"),
        Variant::Int(2),
        "travel(\"jump\") must queue the two-hop path idle -> run -> jump"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "hop0"),
        Variant::String("run".to_string()),
        "first hop"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "hop1"),
        Variant::String("jump".to_string()),
        "second hop"
    );
    // Nothing has stepped yet: travel queues, it does not teleport.
    assert_eq!(
        current_state(ml.tree(), anim_tree_node),
        "idle",
        "travel() must not jump the machine — the hops are walked by step()"
    );

    let dt = 1.0 / 60.0;

    // -- hop 1: the first step takes idle -> run, crossfading over 0.2s --------
    ml.step(dt);
    assert_eq!(
        current_state(ml.tree(), anim_tree_node),
        "run",
        "the first step takes the first queued hop"
    );
    let y_fading = num_prop(ml.tree(), sprite, "y");
    assert!(
        y_fading > 50.0,
        "mid-crossfade Sprite:y must still be blending down from idle(100) toward run(50), got {y_fading}"
    );

    // Read the fade back through the script rather than the graph: one more
    // step keeps us well inside the 0.2s (12-frame) crossfade, so the object
    // still has a fade to report.
    ml.step(dt);
    assert_eq!(
        script_var(ml.tree(), scene_root, "fading_from"),
        Variant::String("idle".to_string()),
        "get_fading_from_node() names the state being faded out of"
    );

    // -- hop 2: once the first fade completes, the queue advances --------------
    // The fade ends around frame 13; 20 more gives margin for the second hop.
    for _ in 0..20 {
        ml.step(dt);
    }
    assert_eq!(
        current_state(ml.tree(), anim_tree_node),
        "jump",
        "the walk continues to the target once the first crossfade completes"
    );

    // -- arrival: the jump clip owns the pose, and the script reads it back ----
    for _ in 0..20 {
        ml.step(dt);
    }
    assert_eq!(
        script_var(ml.tree(), scene_root, "cur"),
        Variant::String("jump".to_string()),
        "get_current_node() reports the travel target after arrival"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "playing"),
        Variant::Bool(true),
        "is_playing() stays true through a travel"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "fading_from"),
        Variant::String(String::new()),
        "no fade is in flight once the walk has arrived"
    );

    let y_end = num_prop(ml.tree(), sprite, "y");
    assert!(
        y_end < 1.0,
        "after arriving the jump pose owns Sprite:y (idle 100 / run 50 / jump 0), got {y_end}"
    );
    let x_end = num_prop(ml.tree(), sprite, "x");
    assert!(
        x_end > 75.0,
        "the jump clip (0->500 over 2s) drives Sprite:x past both other clips' reach in this run (idle <= 10, run ~22), got {x_end}"
    );
}

/// `start()` is the hard cut Godot documents: it teleports without walking the
/// transition graph, so it reaches a state that `travel` would need two hops
/// for — and it discards any queued travel rather than resuming it.
///
/// `stop()` then freezes the machine: `is_playing()` goes false and the pose
/// stops advancing even though `MainLoop::step` keeps running.
#[test]
fn playback_start_teleports_and_stop_freezes_the_machine() {
    let scene = PackedScene::from_tscn(TRAVEL_SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let sprite = tree
        .get_node_or_null(scene_root, "Sprite")
        .expect("Sprite node");
    let anim_tree_node = tree
        .get_node_or_null(scene_root, "AnimationTree")
        .expect("AnimationTree node");

    let script_src = "\
extends Node2D
var frame = 0
var after_start = \"?\"
var queued_after_start = -1
var playing = true
func _ready():
    var p = get_node(\"AnimationTree\").get(\"parameters/playback\")
    p.travel(\"jump\")
    p.start(\"jump\", true)
    self.after_start = p.get_current_node()
    self.queued_after_start = p.get_travel_path().size()
func _process(delta):
    self.frame = self.frame + 1
    var p = get_node(\"AnimationTree\").get(\"parameters/playback\")
    if self.frame == 3:
        p.stop()
    self.playing = p.is_playing()
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    ml.tree_mut().process_script_ready(scene_root);

    // start() teleported past `run` with no step at all, and dropped the travel.
    assert_eq!(
        script_var(ml.tree(), scene_root, "after_start"),
        Variant::String("jump".to_string()),
        "start() must teleport immediately, not queue a walk"
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "queued_after_start"),
        Variant::Int(0),
        "start() discards a queued travel — it is a hard cut, not a resume"
    );
    assert_eq!(
        current_state(ml.tree(), anim_tree_node),
        "jump",
        "the graph itself is on the started state"
    );

    let dt = 1.0 / 60.0;
    // Frames 1-2 advance the jump clip; frame 3 stops it.
    for _ in 0..3 {
        ml.step(dt);
    }
    assert_eq!(
        script_var(ml.tree(), scene_root, "playing"),
        Variant::Bool(false),
        "is_playing() is false after stop()"
    );
    let x_stopped = num_prop(ml.tree(), sprite, "x");

    for _ in 0..30 {
        ml.step(dt);
    }
    let x_later = num_prop(ml.tree(), sprite, "x");
    assert!(
        (x_later - x_stopped).abs() < 1e-6,
        "a stopped playback must freeze the pose: x was {x_stopped}, is {x_later} 30 frames later"
    );
}
