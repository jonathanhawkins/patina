//! pat-s3rrv.45 (merged findings): the `AnimationNodeOneShot` and
//! `AnimationNodeAnimation` parameter surfaces.
//!
//! The audit found both classes stripped down to the one path Patina used:
//!
//! * `OneShot` carried base/shot/fades/length/trigger only —
//!   `grep -rn 'autorestart|mix_mode|break_loop_at_end' crates/` returned **0
//!   hits each**, so a shot always blended (never added), never auto-restarted,
//!   and published nothing a script could poll.
//! * `AnimationNode::Animation` was `{ name, position }` — no `play_mode`, and
//!   `use_custom_timeline` / `timeline_length` / `stretch_time_scale` /
//!   `start_offset` returned 0 hits too, so every leaf played its whole clip
//!   forwards no matter what the scene said.
//!
//! Each case here loads a `.tscn` carrying the property and drives real
//! `MainLoop::step` frames, so the `.tscn` builder and the evaluator are both
//! on the hook.

use gdscene::animation::Animation;
use gdscene::animation_tree::{AnimationNode, AnimationTree, OneShot, OneShotMixMode};
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A clip holding `Sprite:y` at one constant, so a blend or an add is
/// arithmetically identifiable.
fn flat_clip(id: &str, name: &str, value: f32, length: f32) -> String {
    format!(
        r#"[sub_resource type="Animation" id="Animation_{id}"]
resource_name = "{name}"
length = {length:?}
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:y")
tracks/0/interp = 1
tracks/0/keys = {{"times": PackedFloat32Array(0, {length:?}), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [{value:?}, {value:?}]}}

"#
    )
}

/// A clip ramping `Sprite:y` from 0 to 100 across one second, so the sampled
/// value reads back as "how far into the clip are we, in percent".
const RAMP_CLIP: &str = r#"[sub_resource type="Animation" id="Animation_ramp"]
resource_name = "ramp"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:y")
tracks/0/interp = 1
tracks/0/keys = {"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [0.0, 100.0]}

"#;

const NODES: &str = r#"[node name="Character" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
y = 0.0

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("Root")
anim_player = NodePath("../AnimationPlayer")
active = true
"#;

fn num_prop(tree: &SceneTree, node: NodeId, prop: &str) -> f64 {
    match tree.get_node(node).expect("node exists").get_property(prop) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{prop} is not numeric: {other:?}"),
    }
}

/// Instances a scene, attaches `script` to its root, and returns the loop plus
/// the `Sprite` and `AnimationTree` node ids.
fn boot(scene_src: &str, script: Option<&str>) -> (MainLoop, NodeId, NodeId) {
    let scene = PackedScene::from_tscn(scene_src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let sprite = tree
        .get_node_or_null(scene_root, "Sprite")
        .expect("Sprite node");
    let anim_tree = tree
        .get_node_or_null(scene_root, "AnimationTree")
        .expect("AnimationTree node");
    if let Some(src) = script {
        let script = GDScriptNodeInstance::from_source(src, scene_root).expect("script parses");
        tree.attach_script(scene_root, Box::new(script));
    }
    (MainLoop::new(tree), sprite, anim_tree)
}

fn param(ml: &MainLoop, anim_tree: NodeId, path: &str) -> Option<Variant> {
    ml.tree()
        .get_animation_tree(anim_tree)
        .expect("AnimationTree attached")
        .get_parameter(path)
}

// ---------------------------------------------------------------------------
// OneShot: mix_mode, published active/internal_active, autorestart
// ---------------------------------------------------------------------------

/// `base` holds y at 100, `hit` at 10, and the shot is `MIX_MODE_ADD` with no
/// fades, so a firing shot reads 110 rather than the 10 a blend would give.
/// `autorestart` fires it again a quarter-second after it ends.
fn one_shot_scene() -> String {
    let mut s = String::from("[gd_scene load_steps=8 format=3]\n\n");
    s.push_str(&flat_clip("base", "base", 100.0, 1.0));
    s.push_str(&flat_clip("hit", "hit", 10.0, 0.5));
    s.push_str(
        "[sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_main\"]\n_data = {\"base\": SubResource(\"Animation_base\"), \"hit\": SubResource(\"Animation_hit\")}\n\n",
    );
    s.push_str(
        r#"[sub_resource type="AnimationNodeAnimation" id="A"]
animation = &"base"

[sub_resource type="AnimationNodeAnimation" id="B"]
animation = &"hit"

[sub_resource type="AnimationNodeOneShot" id="Shot"]
fadein_time = 0.0
fadeout_time = 0.0
mix_mode = 1
autorestart = true
autorestart_delay = 0.25
autorestart_random_delay = 0.0

[sub_resource type="AnimationNodeBlendTree" id="Root"]
nodes/base/node = SubResource("A")
nodes/hit/node = SubResource("B")
nodes/shot/node = SubResource("Shot")
node_connections = ["output", 0, "shot", "shot", 0, "base", "shot", 1, "hit"]

"#,
    );
    s.push_str(NODES);
    s
}

/// **MIX_MODE_ADD, the published `active` parameter, and `autorestart`** — all
/// three in one run, because they are phases of the same shot's life.
#[test]
fn one_shot_mix_mode_add_publishes_active_and_autorestarts() {
    // The Godot idiom for firing a one-shot: set its request parameter.
    // ONE_SHOT_REQUEST_FIRE is 1.
    let script = "\
extends Node2D
func _process(delta):
    get_node(\"AnimationTree\").set(\"parameters/shot/request\", 1)
";
    let (mut ml, sprite, anim_tree) = boot(&one_shot_scene(), Some(script));
    let dt = 0.1;

    // Frame 1: the graph advances before `_process`, so the request is only set
    // at the end of it. Nothing has fired yet — the base plays alone.
    ml.step(dt);
    assert!(
        (num_prop(ml.tree(), sprite, "y") - 100.0).abs() < 1e-3,
        "before the shot fires only the base plays"
    );
    assert_eq!(
        param(&ml, anim_tree, "parameters/shot/active"),
        Some(Variant::Bool(false)),
        "the one-shot must publish `active` even before it ever fires — a \
         parameter that stays Nil until the first fire is unreadable"
    );

    // Frame 2: the request's rising edge fires the shot, and it is added on top
    // of the base. A MIX_MODE_BLEND shot would read 10 here.
    ml.step(dt);
    assert!(
        (num_prop(ml.tree(), sprite, "y") - 110.0).abs() < 1e-3,
        "MIX_MODE_ADD layers the shot (10) onto the base (100), got {}",
        num_prop(ml.tree(), sprite, "y")
    );
    assert_eq!(
        param(&ml, anim_tree, "parameters/shot/active"),
        Some(Variant::Bool(true)),
        "`active` is true while the shot blends"
    );

    // The shot's hold is its clip's length (0.5s). Four more frames take it
    // past the end, so it stops and the base plays alone again.
    for _ in 0..4 {
        ml.step(dt);
    }
    assert!(
        (num_prop(ml.tree(), sprite, "y") - 100.0).abs() < 1e-3,
        "the shot ends after its clip length"
    );
    assert_eq!(
        param(&ml, anim_tree, "parameters/shot/active"),
        Some(Variant::Bool(false)),
        "`active` drops when the shot ends"
    );
    assert_eq!(
        param(&ml, anim_tree, "parameters/shot/internal_active"),
        Some(Variant::Bool(true)),
        "`internal_active` stays set through the autorestart delay — that is \
         the whole difference between the two parameters"
    );

    // 0.25s of autorestart delay: three frames of 0.1 take it past.
    for _ in 0..3 {
        ml.step(dt);
    }
    assert!(
        (num_prop(ml.tree(), sprite, "y") - 110.0).abs() < 1e-3,
        "autorestart must fire the shot again after its delay, got {}",
        num_prop(ml.tree(), sprite, "y")
    );
    assert_eq!(
        param(&ml, anim_tree, "parameters/shot/active"),
        Some(Variant::Bool(true)),
        "`active` is set again by the autorestart"
    );
}

/// `break_loop_at_end` lets a looping shot finish the loop it is in rather than
/// being cut off at `shot_length`.
///
/// Driven through the Rust API rather than a scene on purpose: the `.tscn`
/// builder sizes `shot_length` from the shot clip itself, so the two are equal
/// there and the flag can make no difference. It bites when a shorter hold was
/// set explicitly, which is what this pins.
#[test]
fn one_shot_break_loop_at_end_extends_the_hold_to_a_whole_loop() {
    let leaf = |name: &str| AnimationNode::Animation {
        name: name.to_string(),
        position: 0.0,
        opts: Default::default(),
    };

    let mut tree = AnimationTree::new();
    tree.add_animation(Animation::new("base", 1.0));
    tree.add_animation(Animation::new("hit", 0.5));

    let cut = OneShot::new(leaf("base"), leaf("hit"))
        .with_shot_length(0.3)
        .with_mix_mode(OneShotMixMode::Blend);
    assert!(
        (cut.total_duration() - 0.3).abs() < 1e-6,
        "without the flag the hold is exactly the configured shot length"
    );

    let mut whole = cut.clone();
    whole.break_loop_at_end = true;
    tree.set_root(AnimationNode::OneShot(whole));
    // One advance resolves the hold against the clip library.
    tree.advance(0.05);
    let AnimationNode::OneShot(resolved) = tree.root.as_ref().expect("root set") else {
        panic!("root is not a one-shot");
    };
    assert!(
        (resolved.total_duration() - 0.5).abs() < 1e-6,
        "break_loop_at_end rounds a 0.3s hold up to the shot clip's 0.5s loop, got {}",
        resolved.total_duration()
    );
}

// ---------------------------------------------------------------------------
// AnimationNodeAnimation: play_mode and the custom timeline
// ---------------------------------------------------------------------------

/// A scene whose whole tree is one `AnimationNodeAnimation` leaf over the ramp
/// clip, with whatever leaf properties the case needs.
fn leaf_scene(leaf_props: &str) -> String {
    let mut s = String::from("[gd_scene load_steps=5 format=3]\n\n");
    s.push_str(RAMP_CLIP);
    s.push_str(
        "[sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_main\"]\n_data = {\"ramp\": SubResource(\"Animation_ramp\")}\n\n",
    );
    s.push_str(&format!(
        "[sub_resource type=\"AnimationNodeAnimation\" id=\"Root\"]\nanimation = &\"ramp\"\n{leaf_props}\n"
    ));
    s.push_str(NODES);
    s
}

/// After 0.1s of a 1s ramp, a forward leaf reads 10 and a `play_mode = 1`
/// (BACKWARD) leaf reads 90 — it starts at the end of the clip and runs back.
#[test]
fn animation_leaf_play_mode_backward_runs_the_clip_in_reverse() {
    let (mut forward, sprite_f, _) = boot(&leaf_scene(""), None);
    forward.step(0.1);
    assert!(
        (num_prop(forward.tree(), sprite_f, "y") - 10.0).abs() < 1e-3,
        "the control: a default leaf is 10% into the ramp after 0.1s, got {}",
        num_prop(forward.tree(), sprite_f, "y")
    );

    let (mut backward, sprite_b, _) = boot(&leaf_scene("play_mode = 1"), None);
    backward.step(0.1);
    assert!(
        (num_prop(backward.tree(), sprite_b, "y") - 90.0).abs() < 1e-3,
        "PLAY_MODE_BACKWARD samples 0.1s in from the END of the clip (90), got {}",
        num_prop(backward.tree(), sprite_b, "y")
    );
}

/// `use_custom_timeline` plays a window of the clip instead of all of it:
/// `start_offset = 0.5` starts halfway in, so 0.1s later the pose is 60%, not
/// 10%.
#[test]
fn animation_leaf_custom_timeline_offsets_the_window() {
    let (mut ml, sprite, _) = boot(
        &leaf_scene(
            "use_custom_timeline = true\ntimeline_length = 0.5\nstart_offset = 0.5\nloop_mode = 1",
        ),
        None,
    );
    ml.step(0.1);
    assert!(
        (num_prop(ml.tree(), sprite, "y") - 60.0).abs() < 1e-3,
        "a window starting at 0.5s reads 60 after 0.1s, got {}",
        num_prop(ml.tree(), sprite, "y")
    );
}

/// `stretch_time_scale` fits the whole clip into `timeline_length`, so a 1s
/// clip on a 0.5s timeline plays at double speed: 0.1s in reads 20%, not 10%.
#[test]
fn animation_leaf_stretch_time_scale_fits_the_clip_to_the_timeline() {
    let (mut ml, sprite, _) = boot(
        &leaf_scene(
            "use_custom_timeline = true\ntimeline_length = 0.5\nstretch_time_scale = true\nloop_mode = 1",
        ),
        None,
    );
    ml.step(0.1);
    assert!(
        (num_prop(ml.tree(), sprite, "y") - 20.0).abs() < 1e-3,
        "a 1s clip stretched onto a 0.5s timeline is 20% in after 0.1s, got {}",
        num_prop(ml.tree(), sprite, "y")
    );
}
