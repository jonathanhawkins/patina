//! pat-s3rrv.1 (merged finding 2): the full
//! `AnimationNodeStateMachineTransition` field surface.
//!
//! The audit found the transition reduced to two of its nine properties —
//! `xfade_time` and `advance_condition`. `switch_mode`, `advance_mode`,
//! `priority`, `reset`, `break_loop_at_end`, `advance_expression` and
//! `xfade_curve` were parsed by nothing, so a scene that set them got the same
//! behaviour as one that did not.
//!
//! The one that changes what a real project does is **`advance_mode`**. Godot's
//! doc XML is explicit: `ADVANCE_MODE_ENABLED` (the default) means "Only use
//! this transition during `travel()`", and only `ADVANCE_MODE_AUTO` advances on
//! a condition. Patina took ANY edge whose condition parameter was true, so a
//! state machine would fire transitions the project had deliberately reserved
//! for `travel()`.
//!
//! Every case here builds the machine from a `.tscn`, so the parser and the
//! evaluator are both on the hook.

use gdscene::animation_tree::AnimationNode;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// Two clips holding `Sprite:y` at distinct constants, so the active state is
/// readable as a number.
const CLIPS: &str = r#"[sub_resource type="Animation" id="Animation_idle"]
resource_name = "idle"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:y")
tracks/0/interp = 1
tracks/0/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [100.0, 100.0]}

[sub_resource type="Animation" id="Animation_run"]
resource_name = "run"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:y")
tracks/0/interp = 1
tracks/0/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [50.0, 50.0]}

[sub_resource type="Animation" id="Animation_jump"]
resource_name = "jump"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:y")
tracks/0/interp = 1
tracks/0/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [10.0, 10.0]}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {"idle": SubResource("Animation_idle"), "run": SubResource("Animation_run"), "jump": SubResource("Animation_jump")}

[sub_resource type="AnimationNodeAnimation" id="Anim_idle"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="Anim_run"]
animation = &"run"

[sub_resource type="AnimationNodeAnimation" id="Anim_jump"]
animation = &"jump"
"#;

const NODES: &str = r#"[node name="Character" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
y = 0.0

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("StateMachine_main")
anim_player = NodePath("../AnimationPlayer")
active = true
"#;

/// Builds a scene whose machine is `idle -> run`, with `transition_props`
/// spliced into the `idle -> run` edge.
fn scene_with_edge(transition_props: &str) -> String {
    format!(
        "[gd_scene load_steps=12 format=3]\n\n{CLIPS}\n\
         [sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"T_start\"]\n\
         xfade_time = 0.0\n\n\
         [sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"T_run\"]\n\
         {transition_props}\n\n\
         [sub_resource type=\"AnimationNodeStateMachine\" id=\"StateMachine_main\"]\n\
         states/idle/node = SubResource(\"Anim_idle\")\n\
         states/run/node = SubResource(\"Anim_run\")\n\
         transitions = [\"Start\", \"idle\", SubResource(\"T_start\"), \"idle\", \"run\", SubResource(\"T_run\")]\n\n\
         {NODES}"
    )
}

fn boot(scene_src: &str) -> (MainLoop, NodeId) {
    let scene = PackedScene::from_tscn(scene_src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let anim_tree = tree
        .get_node_or_null(scene_root, "AnimationTree")
        .expect("AnimationTree node");
    (MainLoop::new(tree), anim_tree)
}

fn current_state(tree: &SceneTree, anim_tree: NodeId) -> String {
    match &tree
        .get_animation_tree(anim_tree)
        .expect("AnimationTree attached")
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

fn set_condition(ml: &mut MainLoop, anim_tree: NodeId, name: &str, value: bool) {
    ml.tree_mut()
        .get_animation_tree_mut(anim_tree)
        .expect("AnimationTree attached")
        .set_parameter(
            format!("parameters/conditions/{name}"),
            Variant::Bool(value),
        );
}

/// **The `advance_mode` acceptance case.** A condition-carrying edge left at
/// Godot's default (`ENABLED`) must NOT fire on its own, and the same edge with
/// `advance_mode = 2` (`AUTO`) must.
///
/// Both halves are asserted because only the pair discriminates: before this
/// bead every edge behaved like AUTO, so testing AUTO alone would have passed
/// against the bug.
#[test]
fn advance_mode_gates_automatic_transitions() {
    // (a) ENABLED — Godot's default. `travel()`-only, so the condition is inert.
    let (mut ml, anim_tree) = boot(&scene_with_edge(
        "xfade_time = 0.0\nadvance_condition = &\"run\"",
    ));
    set_condition(&mut ml, anim_tree, "run", true);
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        current_state(ml.tree(), anim_tree),
        "idle",
        "ADVANCE_MODE_ENABLED must not auto-advance, however true the condition"
    );

    // (b) AUTO — the same scene with the one property a project sets when it
    // wants condition-driven advancement.
    let (mut ml, anim_tree) = boot(&scene_with_edge(
        "xfade_time = 0.0\nadvance_condition = &\"run\"\nadvance_mode = 2",
    ));
    set_condition(&mut ml, anim_tree, "run", true);
    ml.step(1.0 / 60.0);
    assert_eq!(
        current_state(ml.tree(), anim_tree),
        "run",
        "ADVANCE_MODE_AUTO fires the edge once its condition holds"
    );
}

/// `ADVANCE_MODE_DISABLED` removes the edge from the graph entirely — it is not
/// even a route for `travel()`.
#[test]
fn disabled_edges_are_not_travel_routes() {
    let (mut ml, anim_tree) = boot(&scene_with_edge(
        "xfade_time = 0.0\nadvance_condition = &\"run\"\nadvance_mode = 0",
    ));
    let travelled = ml
        .tree_mut()
        .get_animation_tree_mut(anim_tree)
        .expect("AnimationTree attached")
        .state_machine_mut("")
        .map(|sm| sm.travel("run"))
        .unwrap_or(false);
    assert!(
        !travelled,
        "a DISABLED edge is not a path, so travel() cannot use it"
    );
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(current_state(ml.tree(), anim_tree), "idle");
}

/// An `advance_expression` this engine cannot evaluate must hold the edge shut
/// rather than being treated as satisfied.
///
/// Firing it would be the dangerous direction: the project wrote an expression
/// precisely to gate the transition, and "expression missing, so assume true"
/// silently ignores that gate.
#[test]
fn an_unevaluated_advance_expression_holds_the_edge() {
    let (mut ml, anim_tree) = boot(&scene_with_edge(
        "xfade_time = 0.0\nadvance_mode = 2\nadvance_expression = \"speed > 10\"",
    ));
    for _ in 0..10 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        current_state(ml.tree(), anim_tree),
        "idle",
        "an expression Patina cannot evaluate must not be assumed true"
    );
}

/// With several eligible edges, the **lowest `priority`** wins.
#[test]
fn lowest_priority_edge_wins() {
    // idle has two AUTO edges whose conditions are both satisfied: one to `run`
    // at priority 5, one to `jump` at priority 1.
    let src = format!(
        "[gd_scene load_steps=14 format=3]\n\n{CLIPS}\n\
         [sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"T_start\"]\n\
         xfade_time = 0.0\n\n\
         [sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"T_run\"]\n\
         xfade_time = 0.0\nadvance_mode = 2\npriority = 5\n\n\
         [sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"T_jump\"]\n\
         xfade_time = 0.0\nadvance_mode = 2\npriority = 1\n\n\
         [sub_resource type=\"AnimationNodeStateMachine\" id=\"StateMachine_main\"]\n\
         states/idle/node = SubResource(\"Anim_idle\")\n\
         states/run/node = SubResource(\"Anim_run\")\n\
         states/jump/node = SubResource(\"Anim_jump\")\n\
         transitions = [\"Start\", \"idle\", SubResource(\"T_start\"), \"idle\", \"run\", SubResource(\"T_run\"), \"idle\", \"jump\", SubResource(\"T_jump\")]\n\n\
         {NODES}"
    );
    let (mut ml, anim_tree) = boot(&src);
    ml.step(1.0 / 60.0);
    assert_eq!(
        current_state(ml.tree(), anim_tree),
        "jump",
        "priority 1 must beat priority 5, even though `run` is declared first"
    );
}

/// `switch_mode = 1` (SYNC) carries the source's playback position onto the
/// destination instead of restarting it, which is what keeps a walk→run swap in
/// stride.
#[test]
fn sync_switch_mode_carries_the_playhead() {
    fn position_after(props: &str, frames: usize) -> f64 {
        let (mut ml, anim_tree) = boot(&scene_with_edge(props));
        // Let `idle` run well into its clip before the edge fires.
        set_condition(&mut ml, anim_tree, "run", false);
        for _ in 0..frames {
            ml.step(1.0 / 60.0);
        }
        set_condition(&mut ml, anim_tree, "run", true);
        ml.step(1.0 / 60.0);
        assert_eq!(current_state(ml.tree(), anim_tree), "run");
        match &ml
            .tree()
            .get_animation_tree(anim_tree)
            .expect("AnimationTree attached")
            .root
        {
            // The `.tscn` builder wraps each state in `Named` so its own
            // `parameters/<state>/...` are addressable, so unwrap before
            // reading the clip's playhead.
            Some(AnimationNode::StateMachine(sm)) => match sm.states.get("run") {
                Some(AnimationNode::Named { inner, .. }) => match inner.as_ref() {
                    AnimationNode::Animation { position, .. } => *position,
                    other => panic!("run state does not wrap a clip: {other:?}"),
                },
                Some(AnimationNode::Animation { position, .. }) => *position,
                other => panic!("run state is not a clip: {other:?}"),
            },
            other => panic!("tree_root is not a state machine: {other:?}"),
        }
    }

    // 30 frames ≈ 0.5s into a 1s clip.
    let immediate = position_after(
        "xfade_time = 0.0\nadvance_condition = &\"run\"\nadvance_mode = 2",
        30,
    );
    let synced = position_after(
        "xfade_time = 0.0\nadvance_condition = &\"run\"\nadvance_mode = 2\nswitch_mode = 1",
        30,
    );
    assert!(
        immediate < 0.05,
        "IMMEDIATE restarts the destination from zero, got {immediate}"
    );
    assert!(
        synced > 0.4,
        "SYNC carries the source's ~0.5s playhead across, got {synced}"
    );
}

/// The same scene, but with the `idle` clip's loop mode forced.
///
/// ⚠️ Anchored on `resource_name = "idle"` because `loop_mode = 1` appears
/// three times in `CLIPS` — a first-occurrence replace would happen to hit the
/// right block today and silently retarget the moment a clip is added above it.
fn scene_with_idle_loop(transition_props: &str, idle_loops: bool) -> String {
    let scene = scene_with_edge(transition_props);
    if idle_loops {
        return scene;
    }
    let anchor = "resource_name = \"idle\"\nlength = 1.0\nloop_mode = 1";
    assert_eq!(
        scene.matches(anchor).count(),
        1,
        "the idle-clip anchor must be unique, or this rewrites the wrong clip"
    );
    scene.replace(
        anchor,
        "resource_name = \"idle\"\nlength = 1.0\nloop_mode = 0",
    )
}

/// Steps well past several clip lengths, so "did not transition" means it
/// really holds rather than merely being slow.
fn run(ml: &mut MainLoop, seconds: f64) {
    let steps = (seconds * 60.0).round() as usize;
    for _ in 0..steps {
        ml.step(1.0 / 60.0);
    }
}

/// **`break_loop_at_end`, the flag that was parsed and then consulted by
/// nothing** (pat-s3rrv.1).
///
/// Godot gates an `AT_END` edge on the source's *remaining time*
/// (`animation_node_state_machine.cpp:1037`), and that helper
/// (`animation_tree.h:81 NodeTimeInfo::get_remain`) returns `HUGE_LENGTH` for a
/// looping clip unless `break_loop_at_end` is set:
///
/// ```cpp
/// if ((is_looping() && !p_break_loop) || is_infinity) { return HUGE_LENGTH; }
/// ```
///
/// So a looping state with an `AT_END` edge **holds forever** at Godot's
/// default. Patina left after one cycle, because the gate only asked whether
/// the playhead had wrapped — and Patina's default leaf wraps with `rem_euclid`
/// no matter what the clip's own loop mode says, so a wrap could never
/// distinguish a looping clip from a one-shot one.
///
/// `false` is the DEFAULT, so this was not an edge case: every looping
/// idle/run state with an `AT_END` edge cut away on its own.
#[test]
fn at_end_holds_a_looping_source_until_break_loop_at_end() {
    // AUTO + AT_END, and no `break_loop_at_end` — exactly what Godot writes by
    // default.
    //
    // ⚠️ The condition is carried AND satisfied on purpose. Without it a pass
    // here would be ambiguous: "still idle" could mean the AT_END gate held the
    // edge (what is being tested) or that an unconditional AUTO edge simply
    // never fires (which would make this test vacuous). Satisfying the
    // condition leaves the AT_END gate as the only thing that can hold it.
    let (mut ml, anim_tree) = boot(&scene_with_idle_loop(
        "xfade_time = 0.0\nswitch_mode = 2\nadvance_mode = 2\nadvance_condition = &\"go\"",
        true,
    ));
    set_condition(&mut ml, anim_tree, "go", true);
    run(&mut ml, 5.0);
    assert_eq!(
        current_state(ml.tree(), anim_tree),
        "idle",
        "a LOOPING source with an AT_END edge and break_loop_at_end = false \
         must never advance — five clip lengths in, it is still idle in Godot. \
         Reading `run` here means the edge fired on the first wrap, which is \
         the pre-fix behaviour."
    );
}

#[test]
fn break_loop_at_end_releases_the_looping_source() {
    // The same edge with the same satisfied condition, flag set: now the loop
    // is broken at the end of a cycle. This is the half that proves the test
    // above is not passing merely because the edge was never eligible.
    let (mut ml, anim_tree) = boot(&scene_with_idle_loop(
        "xfade_time = 0.0\nswitch_mode = 2\nadvance_mode = 2\nadvance_condition = &\"go\"\nbreak_loop_at_end = true",
        true,
    ));
    set_condition(&mut ml, anim_tree, "go", true);
    run(&mut ml, 5.0);
    assert_eq!(
        current_state(ml.tree(), anim_tree),
        "run",
        "break_loop_at_end = true must let the AT_END edge fire at the end of a \
         cycle. Still `idle` means the flag is parsed and stored but never \
         consulted — which is exactly what it did before this fix."
    );
}

/// ⚠️ The negative control that stops the fix above from going too far.
///
/// `break_loop_at_end` is meaningless for a NON-looping clip: upstream's guard
/// is `is_looping() && !p_break_loop`, so a one-shot source falls straight
/// through to `length - position`. Gating on the flag alone — rather than on
/// "looping AND not flagged" — would make `AT_END` dead for every one-shot
/// clip, which is the *common* case the mode exists for: "attack → idle when
/// the clip ends".
#[test]
fn at_end_still_fires_for_a_non_looping_source() {
    let (mut ml, anim_tree) = boot(&scene_with_idle_loop(
        "xfade_time = 0.0\nswitch_mode = 2\nadvance_mode = 2\nadvance_condition = &\"go\"",
        false,
    ));
    set_condition(&mut ml, anim_tree, "go", true);
    run(&mut ml, 5.0);
    assert_eq!(
        current_state(ml.tree(), anim_tree),
        "run",
        "a NON-looping source must still advance on AT_END regardless of \
         break_loop_at_end. `idle` here means the new looping gate is being \
         applied to one-shot clips too, which would break the mode's main use."
    );
}
