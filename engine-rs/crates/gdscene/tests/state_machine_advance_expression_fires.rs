//! pat-mgixk.82 acceptance: a StateMachine's `advance_expression` is evaluated.
//!
//! # What was wrong
//!
//! `animation_tree.rs`'s auto-advance loop did `continue` for ANY transition
//! carrying a non-empty `advance_expression`, so such an edge could never fire —
//! `animation_tree_tscn.rs` parsed the string and nothing ever evaluated it. The
//! skip was deliberate and its reasoning was sound ("an expression this engine
//! cannot evaluate must not be treated as satisfied"); what was missing was the
//! evaluator. The field `advance_expression_base_node`, which says what the
//! expression's barewords resolve against, was not parsed at all.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> PackedScene::from_tscn -> add_packed_scene_to_tree
//!   -> attached GDScript sets `speed` on the base node
//!     -> MainLoop::step -> process_animation_trees
//!       -> SceneTree::advance_expression_scope   (snapshot of the base node)
//!         -> AnimationTree::advance_with_expressions
//!           -> select_auto_transition
//!             -> advance_expression::eval_advance_expression
//! ```
//!
//! ⚠️ BOTH halves are asserted, and only the pair discriminates. Before this
//! bead the edge was skipped unconditionally, so the "stays idle" half passes
//! against the bug too — it is here to prove the gate still CLOSES, i.e. that
//! the fix did not simply let every expression edge through. The "fires" half is
//! the one that fails at HEAD.

use gdscene::animation_tree::AnimationNode;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::MainLoop;

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

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {"idle": SubResource("Animation_idle"), "run": SubResource("Animation_run")}

[sub_resource type="AnimationNodeAnimation" id="Anim_idle"]
animation = &"idle"

[sub_resource type="AnimationNodeAnimation" id="Anim_run"]
animation = &"run"
"#;

/// The `Player` node is the expression's base: `advance_expression_base_node`
/// points at it, so `speed` in the expression resolves to ITS member, not to one
/// on the AnimationTree.
///
/// ⚠️ `speed` is deliberately NOT authored on the node — only the attached
/// script declares it. One home for the value means the test cannot pass (or
/// fail) for the wrong reason: an engine property and a script member of the same
/// name would leave the assertion depending on which of the two the snapshot
/// preferred, rather than on whether the expression was evaluated at all.
const NODES: &str = r#"[node name="Character" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
y = 0.0

[node name="Player" type="Node2D" parent="."]

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("StateMachine_main")
anim_player = NodePath("../AnimationPlayer")
advance_expression_base_node = NodePath("../Player")
active = true
"#;

/// Declares `speed` and pushes it past the expression's threshold on demand.
///
/// The assignment is written bare rather than `self.`-qualified: the
/// interpreter's assignment path checks `self_instance.properties` for the name
/// first (`gdscript-interop/src/interpreter.rs:2262`), so a bare write to a
/// declared `var` is the spelling that is certain to land on the instance member
/// the snapshot reads.
const DRIVER: &str = "\
extends Node2D
var speed = 0.0
func go():
\tspeed = 2.0
";

/// The machine is `idle -> run` with the `idle -> run` edge gated on
/// `advance_expression`. `advance_mode = 2` is AUTO — without it the edge is
/// `travel()`-only however true the expression is, which the sibling
/// `state_machine_transition_surface` pins.
fn scene() -> String {
    format!(
        "[gd_scene load_steps=13 format=3]\n\n{CLIPS}\n\
         [sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"T_start\"]\n\
         xfade_time = 0.0\n\n\
         [sub_resource type=\"Curve\" id=\"Curve_ease\"]\n\
         _data = [Vector2(0, 0), 0.0, 0.0, 0, 0, Vector2(1, 1), 2.0, 0.0, 0, 0]\n\n\
         [sub_resource type=\"AnimationNodeStateMachineTransition\" id=\"T_run\"]\n\
         xfade_time = 2.0\n\
         xfade_curve = SubResource(\"Curve_ease\")\n\
         advance_mode = 2\n\
         advance_expression = \"speed > 1.0\"\n\n\
         [sub_resource type=\"AnimationNodeStateMachine\" id=\"StateMachine_main\"]\n\
         states/idle/node = SubResource(\"Anim_idle\")\n\
         states/run/node = SubResource(\"Anim_run\")\n\
         transitions = [\"Start\", \"idle\", SubResource(\"T_start\"), \"idle\", \"run\", SubResource(\"T_run\")]\n\n\
         {NODES}"
    )
}

struct Fixture {
    main_loop: MainLoop,
    anim_tree: NodeId,
    player: NodeId,
    /// The node both clips animate, so the crossfade weight is observable as a
    /// property value (pat-mgixk.82).
    sprite: NodeId,
}

/// `idle` holds `Sprite:y` here.
const IDLE_Y: f32 = 100.0;
/// `run` holds it here, so a crossfade at weight `w` yields
/// `IDLE_Y + (RUN_Y - IDLE_Y) * w`.
const RUN_Y: f32 = 50.0;

/// What `Sprite:y` must read at the half-way point of the crossfade, with the
/// authored `xfade_curve` applied.
///
/// 🔑 Hand-derived from Godot's own formula, NOT from Patina's output, so this
/// is not the implementation checked against itself. `Curve::sample`
/// (`scene/resources/curve.cpp`, `sample_local_nocheck`) is a cubic Bezier:
/// with points `(0,0)` and `(1,1)`, `d = 1`, `d/3` and the authored tangents
/// `a.right_tangent = 0` / `b.left_tangent = 2`, the control values are
/// `yac = 0 + (1/3)·0 = 0` and `ybc = 1 − (1/3)·2 = 1/3`. Then
/// `bezier(0, 0, 1/3, 1, 0.5)`
/// `= 0·0.125 + 0·0.375 + (1/3)·0.375 + 1·0.125 = 0.25`.
///
/// So the weight is **0.25**, not the linear 0.5, and
/// `y = 100 + (50 − 100)·0.25 = 87.5`.
const CURVED_MID_Y: f32 = 87.5;

/// What it would read if the curve were ignored: `100 + (50 − 100)·0.5`.
///
/// ⚠️ Named and asserted against, because this is the pre-bead value. The two
/// are 12.5 apart, so the tolerance below cannot span both.
const LINEAR_MID_Y: f32 = 75.0;

fn boot() -> Fixture {
    let scene = PackedScene::from_tscn(&scene()).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let anim_tree = tree
        .get_node_or_null(scene_root, "AnimationTree")
        .expect("AnimationTree node");
    let player = tree
        .get_node_or_null(scene_root, "Player")
        .expect("Player node");
    let script = GDScriptNodeInstance::from_source(DRIVER, player).expect("driver parses");
    tree.attach_script(player, Box::new(script));
    let mut main_loop = MainLoop::new(tree);
    main_loop.tree_mut().process_script_ready(player);
    let sprite = main_loop
        .tree()
        .get_node_or_null(scene_root, "Sprite")
        .expect("Sprite node");
    Fixture {
        main_loop,
        anim_tree,
        player,
        sprite,
    }
}

/// `Sprite:y` as the AnimationTree last wrote it — the same read
/// `animation_tree_runtime_test.rs:104` uses for an applied track value.
fn sprite_y(fx: &Fixture) -> f32 {
    match fx
        .main_loop
        .tree()
        .get_node(fx.sprite)
        .expect("Sprite exists")
        .get_property("y")
    {
        gdvariant::Variant::Float(y) => y as f32,
        gdvariant::Variant::Int(y) => y as f32,
        other => panic!("`Sprite:y` should be a number, got {other:?}"),
    }
}

/// The state machine's current state, read the way the sibling test does.
fn current_state(fx: &Fixture) -> String {
    match &fx
        .main_loop
        .tree()
        .get_animation_tree(fx.anim_tree)
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

fn step(fx: &mut Fixture, frames: usize) {
    for _ in 0..frames {
        fx.main_loop.step(1.0 / 60.0);
    }
}

/// **THE ACCEPTANCE TEST (pat-mgixk.82).**
#[test]
fn state_machine_advance_expression_fires() {
    // -- (a) the gate CLOSES while the expression is false -------------------
    //
    // ⚠️ This half passes at HEAD too, and that is the point: it proves the fix
    // did not turn "cannot evaluate" into "fire anyway". An implementation that
    // simply stopped skipping expression edges would leave idle immediately and
    // fail here.
    let mut fx = boot();
    assert_eq!(
        current_state(&fx),
        "idle",
        "fixture precondition: the machine must start in idle, or neither half \
         below measures the transition",
    );
    step(&mut fx, 30);
    assert_eq!(
        current_state(&fx),
        "idle",
        "`speed` is 0.0, so `speed > 1.0` is FALSE and the AUTO edge must not \
         fire. Leaving idle here means expression edges are being treated as \
         satisfied rather than evaluated",
    );

    // -- (b) …and OPENS once the script sets speed past the threshold --------
    let _ = fx
        .main_loop
        .tree_mut()
        .call_script_method(fx.player, "go", &[]);
    step(&mut fx, 30);
    assert_eq!(
        current_state(&fx),
        "run",
        "after the script sets `speed = 2.0` on the node named by \
         `advance_expression_base_node`, `speed > 1.0` is TRUE and the AUTO edge \
         must fire. Still `idle` is the pre-bead behaviour: the auto-advance loop \
         skipped every transition carrying an advance_expression, so the edge \
         could never fire however true the expression was",
    );

    // -- (c) 🔑 the authored `xfade_curve` SHAPES the crossfade --------------
    //
    // The bead's second acceptance clause, which the committed test never
    // asserted: `xfade_curve` was parsed into a sub-resource id and read by
    // nothing, so every crossfade stayed linear however the scene authored it.
    //
    // Both clips hold `Sprite:y` at a constant, so the blended property IS the
    // crossfade weight, scaled: `y = IDLE_Y + (RUN_Y - IDLE_Y) * w`.
    let mut fx = boot();
    let _ = fx
        .main_loop
        .tree_mut()
        .call_script_method(fx.player, "go", &[]);

    // ⚠️ Stepped ONE frame at a time until the edge fires, rather than assuming
    // which frame that is. The fade is 2.0s, so knowing the start to within one
    // frame bounds the measured weight to 0.500..0.508 — far inside the margin
    // below. Guessing the start frame instead would make this assertion depend
    // on how promptly the expression happens to be evaluated.
    let mut fired = false;
    for _ in 0..30 {
        step(&mut fx, 1);
        if current_state(&fx) == "run" {
            fired = true;
            break;
        }
    }
    assert!(
        fired,
        "the AUTO edge must fire within 30 frames of `go()`, or clause (c) \
         has no crossfade to measure"
    );

    // Half of the authored 2.0s crossfade, at 60 fps.
    step(&mut fx, 60);
    let y = sprite_y(&fx);
    assert!(
        (y - CURVED_MID_Y).abs() < 1.5,
        "at the half-way point of the crossfade `Sprite:y` must be {} \
         (weight 0.25 — see CURVED_MID_Y for the hand-derived Bezier), got \
         {y}. {} means the curve was ignored and the crossfade blended \
         linearly, which is the pre-bead behaviour; a value at or beyond {} \
         means the fade had already completed and this measures nothing",
        CURVED_MID_Y,
        LINEAR_MID_Y,
        RUN_Y
    );
    // And the fade really is mid-flight, not finished or not started: a
    // completed fade reads RUN_Y and an unstarted one IDLE_Y, and either would
    // make the clause above vacuous if the tolerance ever widened.
    assert!(
        y > RUN_Y && y < IDLE_Y,
        "the crossfade must still be in flight at the measurement point, got \
         {y} (outside the open interval between {} and {})",
        RUN_Y,
        IDLE_Y
    );
}
