//! pat-u3l8c.96 — AnimationTree drives at runtime.
//!
//! App-level (integration) tests for the AnimationTree runtime. Before this
//! bead the blend math existed but was never executed against nodes, and the
//! blend-space clocks never advanced (BlendSpace1D/2D sampled at t=0 always).
//!
//! These tests assert the two acceptance criteria via the crate's public API:
//!   1. A state machine drives a property animation on a real SceneTree node.
//!   2. A 1D blend space blends two clips by its position parameter AND plays
//!      them forward over time (the elapsed clock advances).
//!
//! They live in `tests/` (an integration `--test` binary) rather than as
//! `#[cfg(test)]` unit tests so verification compiles `gdscene` as a plain
//! library — mirroring how the sibling `animation_targeting_test` is run.

use gdscene::animation::{Animation, AnimationTrack, KeyFrame};
use gdscene::animation_tree::{
    advance_and_apply, AnimationNode, AnimationTree, BlendSpace1D, StateMachine,
};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// An animation that ramps property `x` linearly from `v0` to `v1` over 1s.
fn ramp_anim(name: &str, v0: f64, v1: f64) -> Animation {
    let mut anim = Animation::new(name, 1.0);
    let mut track = AnimationTrack::new("x");
    track.add_keyframe(KeyFrame::linear(0.0, Variant::Float(v0)));
    track.add_keyframe(KeyFrame::linear(1.0, Variant::Float(v1)));
    anim.tracks.push(track);
    anim
}

fn float_of(values: &[(String, Variant)], prop: &str) -> f64 {
    for (p, v) in values {
        if p == prop {
            if let Variant::Float(f) = v {
                return *f;
            }
        }
    }
    panic!("no float for {prop}");
}

#[test]
fn blend_space_1d_blends_by_position_and_plays_over_time() {
    let mut tree = AnimationTree::new();
    // Two ramp clips: A goes 0->10, B goes 100->110 over 1s.
    tree.add_animation(ramp_anim("a", 0.0, 10.0));
    tree.add_animation(ramp_anim("b", 100.0, 110.0));

    let mut bs = BlendSpace1D::new();
    bs.add_point(0.0, "a");
    bs.add_point(1.0, "b");
    bs.blend_position = 0.5; // midway between the two clips
    tree.set_root(AnimationNode::BlendSpace1D(bs));

    // Blend by position: midway between clip A (start 0) and clip B (start 100).
    let v0 = float_of(&tree.get_current_values(), "x");
    assert!(
        (40.0..=60.0).contains(&v0),
        "blend-by-position at start: v0={v0}"
    );

    // Over time: advancing the tree plays both clips forward (they ramp up),
    // so the blended value increases — previously it was frozen at t=0.
    tree.advance(0.5);
    let v_mid = float_of(&tree.get_current_values(), "x");
    assert!(
        v_mid > v0 + 1.0,
        "blend-space clips must play over time: v0={v0} v_mid={v_mid}"
    );
}

#[test]
fn state_machine_drives_node_property_over_time() {
    let mut scene = SceneTree::new();
    let root = scene.root_id();
    let node = scene.add_child(root, Node::new("N", "Node")).unwrap();

    // A state machine whose current state plays a clip that ramps "x" 0->42.
    let mut tree = AnimationTree::new();
    tree.add_animation(ramp_anim("run", 0.0, 42.0));
    let mut sm = StateMachine::new();
    sm.states.insert(
        "run".into(),
        AnimationNode::Animation {
            name: "run".into(),
            position: 0.0,
        },
    );
    sm.current = "run".into();
    tree.set_root(AnimationNode::StateMachine(sm));

    // Drive the tree at runtime and apply its pose to the node.
    advance_and_apply(&mut tree, &mut scene, node, 0.5);

    // The state machine drove the node's "x" property over time
    // (0 -> 42 over 1s, so ~21 at 0.5s).
    match scene.get_node(node).unwrap().get_property("x") {
        Variant::Float(x) => assert!((x - 21.0).abs() < 1.0, "x={x}"),
        other => panic!("x not driven by AnimationTree: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// pat-0q2kx.2 — a `.tscn` AnimationTree drives nodes from MainLoop::step alone
// ---------------------------------------------------------------------------

/// A scene in Godot's own `.tscn` layout: an `AnimationPlayer` holding an
/// `AnimationLibrary` of two clips, plus an `AnimationTree` whose `tree_root` is
/// an `AnimationNodeStateMachine` over those clips with a conditional,
/// crossfading transition (`advance_condition = &"run"`, `xfade_time = 0.5`).
///
/// Each clip drives two tracks so the test can tell time-advance from blending:
/// `x` ramps (idle 0->10, run 0->100) and `y` is held constant (idle 100, run 0),
/// so the 0.5s crossfade shows up as `y` decaying rather than snapping.
///
/// The `keys` dictionaries are written on one line; the multi-line form Godot
/// emits is exercised by pat-0q2kx.1's own parser tests.
const STATE_MACHINE_SCENE: &str = r#"[gd_scene load_steps=10 format=3]

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
tracks/1/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [0.0, 0.0]}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {"idle": SubResource("Animation_idle"), "run": SubResource("Animation_run")}

[sub_resource type="AnimationNodeAnimation" id="Anim_idle"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="Anim_run"]
animation = &"run"

[sub_resource type="AnimationNodeStateMachineTransition" id="Trans_start"]
xfade_time = 0.0

[sub_resource type="AnimationNodeStateMachineTransition" id="Trans_run"]
xfade_time = 0.5
advance_condition = &"run"

[sub_resource type="AnimationNodeStateMachine" id="StateMachine_main"]
states/idle/node = SubResource("Anim_idle")
states/idle/position = Vector2(300, 100)
states/run/node = SubResource("Anim_run")
states/run/position = Vector2(500, 100)
transitions = ["Start", "idle", SubResource("Trans_start"), "idle", "run", SubResource("Trans_run")]

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
parameters/conditions/run = false
"#;

/// Reads a numeric node property.
fn num_prop(tree: &SceneTree, node: NodeId, prop: &str) -> f64 {
    match tree.get_node(node).expect("node exists").get_property(prop) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{prop} is not numeric: {other:?}"),
    }
}

/// The state machine's current state, read out of the attached tree.
fn current_state(tree: &SceneTree, anim_tree_node: NodeId) -> String {
    match &tree
        .get_animation_tree(anim_tree_node)
        .expect("AnimationTree attached at scene load")
        .root
    {
        Some(AnimationNode::StateMachine(sm)) => sm.current.clone(),
        other => panic!("tree_root is not a state machine: {other:?}"),
    }
}

/// The acceptance test for pat-0q2kx.2: loading a `.tscn` that contains an
/// AnimationPlayer (clips `idle`/`run`) and an active AnimationTree state machine
/// must animate a node from `MainLoop::step` alone — no test-side
/// `attach_animation_tree`, no manual `advance_and_apply` — and a GDScript
/// setting `parameters/conditions/run` must transition states over the
/// transition's declared crossfade.
#[test]
fn animation_tree_tscn_state_machine_drives_node() {
    let scene = PackedScene::from_tscn(STATE_MACHINE_SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    // The ordinary scene-instancing entry point the runner and editor both use.
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let sprite = tree
        .get_node_or_null(scene_root, "Sprite")
        .expect("Sprite node");
    let anim_tree_node = tree
        .get_node_or_null(scene_root, "AnimationTree")
        .expect("AnimationTree node");

    // Scene load built the graph and resolved the clip library from the player at
    // `anim_player` — nothing else in the test wires the tree.
    {
        let anim_tree = tree
            .get_animation_tree(anim_tree_node)
            .expect("scene load must attach an AnimationTree");
        assert!(anim_tree.active, "active = true in the scene");
        assert!(
            anim_tree.animations.contains_key("idle") && anim_tree.animations.contains_key("run"),
            "clip library resolved from anim_player: {:?}",
            anim_tree.animations.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            anim_tree.anim_player, "../AnimationPlayer",
            "anim_player path recorded"
        );
    }
    assert_eq!(
        current_state(&tree, anim_tree_node),
        "idle",
        "the Start transition selects the initial state"
    );

    // A script that flips the state machine's condition on frame 4, exactly as a
    // game would: `tree.set(\"parameters/conditions/run\", true)`.
    let script_src = "\
extends Node2D
var frame = 0
func _process(delta):
    self.frame = self.frame + 1
    if self.frame == 4:
        var t = get_node(\"AnimationTree\")
        t.set(\"parameters/conditions/run\", true)
";
    let script = GDScriptNodeInstance::from_source(script_src, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    let dt = 1.0 / 60.0;

    // -- the idle state's clip advances and writes across frames --------------
    ml.step(dt);
    let x1 = num_prop(ml.tree(), sprite, "x");
    ml.step(dt);
    ml.step(dt);
    let x3 = num_prop(ml.tree(), sprite, "x");
    assert!(
        x1 > 0.0 && x3 > x1,
        "idle clip must advance and write Sprite:x across frames (x1={x1} x3={x3})"
    );
    assert!(
        (num_prop(ml.tree(), sprite, "y") - 100.0).abs() < 1e-3,
        "the idle pose holds Sprite:y at 100"
    );

    // -- frame 4: the script sets the condition; frame 5 takes the transition --
    ml.step(dt); // script sets parameters/conditions/run = true
    ml.step(dt); // animation phase sees it and starts the crossfade
    assert_eq!(
        current_state(ml.tree(), anim_tree_node),
        "run",
        "a true condition takes the idle -> run transition"
    );
    assert_eq!(
        ml.tree()
            .get_animation_tree(anim_tree_node)
            .and_then(|t| t.get_parameter("parameters/conditions/run")),
        Some(Variant::Bool(true)),
        "the script's set() reached the tree's parameter map"
    );

    // -- mid-crossfade: the 0.5s xfade blends rather than snapping -------------
    // ~0.25s into the fade, y sits between the idle pose (100) and the run pose (0).
    for _ in 0..14 {
        ml.step(dt);
    }
    let y_mid = num_prop(ml.tree(), sprite, "y");
    assert!(
        (20.0..80.0).contains(&y_mid),
        "mid-crossfade Sprite:y must blend idle(100) -> run(0), got {y_mid}"
    );

    // -- crossfade complete: the run clip drives alone ------------------------
    for _ in 0..20 {
        ml.step(dt);
    }
    let y_end = num_prop(ml.tree(), sprite, "y");
    let x_end = num_prop(ml.tree(), sprite, "x");
    assert!(
        y_end < 1.0,
        "after the crossfade the run pose owns Sprite:y, got {y_end}"
    );
    assert!(
        x_end > 25.0,
        "the run clip (0->100 over 1s) drives Sprite:x past idle's 0->10 range, got {x_end}"
    );
}
