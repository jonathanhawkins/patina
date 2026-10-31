//! pat-mgixk.76 acceptance — a `.tscn` blend tree containing `TimeScale`,
//! `Add2` and `Transition` actually loads and drives the target node.
//!
//! Godot ground truth: `scene/animation/animation_blend_tree.cpp`
//! (`AnimationNodeAdd2`, `AnimationNodeTimeScale`, `AnimationNodeTransition`),
//! `doc/classes/AnimationNodeTransition.xml` (`transition_request` names an
//! input).
//!
//! ⚠️ THE FAILURE WAS WILDLY OUT OF PROPORTION TO ITS CAUSE. The evaluator
//! already supported all of these — the variants exist and `apply_named_params`
//! already read `parameters/<n>/add_amount`, `/sub_amount`, `/scale` and
//! `/seek_request`. Only the LOADER lacked arms, so an unmatched type fell to
//! the generic conversion, which answers `None`; and because every arm resolves
//! its ports with `input(N)?`, that `None` propagated to the root. **One
//! `TimeScale` node left a real project with no AnimationTree at all** — not a
//! degraded one, none.
//!
//! # Why every assertion here is a differential
//!
//! 🔑 None of these predicts a blend value. Each renders the SAME fixture twice
//! with one parameter changed and asserts the direction of the difference, so
//! nothing depends on my model of the engine's interpolation — only on the
//! engine agreeing with itself. The clips are chosen so each effect moves
//! `Sprite:y` in a direction no other effect could produce.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// A clip that ramps `Sprite:y` from `from` to `to` across its 1s length.
///
/// ⚠️ A RAMP, not a constant: `TimeScale` can only be observed through a value
/// that depends on playback position. With constant clips (which is what every
/// other blend fixture in the tree uses) doubling the speed is invisible.
fn ramp_clip(id: &str, from: f32, to: f32) -> String {
    format!(
        r#"[sub_resource type="Animation" id="Animation_{id}"]
resource_name = "{id}"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:y")
tracks/0/interp = 1
tracks/0/keys = {{"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [{from:?}, {to:?}]}}

"#
    )
}

/// `output <- TS(TimeScale) <- Add(Add2) <- [ramp, flat]`, optionally with an
/// unsupported node spliced in between `TS` and `Add`.
fn add2_timescale_scene(with_unsupported: bool) -> String {
    let mut s = String::from("[gd_scene load_steps=12 format=3]\n\n");
    // `ramp` sweeps 0..100 so position is readable off y; `flat` holds 40 so the
    // additive layer contributes a fixed, recognisable amount.
    s.push_str(&ramp_clip("ramp", 0.0, 100.0));
    s.push_str(&ramp_clip("flat", 40.0, 40.0));
    s.push_str(
        "[sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_main\"]\n\
         _data = {\"ramp\": SubResource(\"Animation_ramp\"), \"flat\": SubResource(\"Animation_flat\")}\n\n",
    );
    for id in ["ramp", "flat"] {
        s.push_str(&format!(
            "[sub_resource type=\"AnimationNodeAnimation\" id=\"Anim_{id}\"]\nanimation = &\"{id}\"\n\n"
        ));
    }
    s.push_str("[sub_resource type=\"AnimationNodeAdd2\" id=\"Add2_1\"]\n\n");
    s.push_str("[sub_resource type=\"AnimationNodeTimeScale\" id=\"TS_1\"]\n\n");
    if with_unsupported {
        // A type the loader has no arm for. Before this bead that made the whole
        // tree `None`; now it passes its single input through.
        s.push_str("[sub_resource type=\"AnimationNodeNotARealType\" id=\"Bogus_1\"]\n\n");
    }

    s.push_str("[sub_resource type=\"AnimationNodeBlendTree\" id=\"Tree\"]\n");
    s.push_str("nodes/TS/node = SubResource(\"TS_1\")\n");
    s.push_str("nodes/Add/node = SubResource(\"Add2_1\")\n");
    s.push_str("nodes/ramp/node = SubResource(\"Anim_ramp\")\n");
    s.push_str("nodes/flat/node = SubResource(\"Anim_flat\")\n");
    if with_unsupported {
        s.push_str("nodes/Bogus/node = SubResource(\"Bogus_1\")\n");
    }
    // `node_connections` is flat triples of (destination, port, source).
    let chain = if with_unsupported {
        "&\"output\", 0, &\"TS\", &\"TS\", 0, &\"Bogus\", &\"Bogus\", 0, &\"Add\", \
         &\"Add\", 0, &\"ramp\", &\"Add\", 1, &\"flat\""
    } else {
        "&\"output\", 0, &\"TS\", &\"TS\", 0, &\"Add\", \
         &\"Add\", 0, &\"ramp\", &\"Add\", 1, &\"flat\""
    };
    s.push_str(&format!("node_connections = [{chain}]\n\n"));

    s.push_str(
        r#"[node name="Character" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
y = 0.0

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("Tree")
anim_player = NodePath("../AnimationPlayer")
active = true
"#,
    );
    s
}

/// `output <- T(Transition over [a, b])`, named inputs, no crossfade.
///
/// ⚠️ `xfade_time = 0.0` on purpose: a non-zero fade makes the value
/// mid-blend for several frames, so the test would be asserting against the
/// fade curve rather than against the switch.
fn transition_scene() -> String {
    let mut s = String::from("[gd_scene load_steps=10 format=3]\n\n");
    s.push_str(&ramp_clip("lo", 10.0, 10.0));
    s.push_str(&ramp_clip("hi", 90.0, 90.0));
    s.push_str(
        "[sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_main\"]\n\
         _data = {\"lo\": SubResource(\"Animation_lo\"), \"hi\": SubResource(\"Animation_hi\")}\n\n",
    );
    for id in ["lo", "hi"] {
        s.push_str(&format!(
            "[sub_resource type=\"AnimationNodeAnimation\" id=\"Anim_{id}\"]\nanimation = &\"{id}\"\n\n"
        ));
    }
    s.push_str(
        "[sub_resource type=\"AnimationNodeTransition\" id=\"T_1\"]\n\
         input_count = 2\n\
         input_0/name = \"a\"\n\
         input_1/name = \"b\"\n\
         xfade_time = 0.0\n\n",
    );
    s.push_str(
        "[sub_resource type=\"AnimationNodeBlendTree\" id=\"Tree\"]\n\
         nodes/T/node = SubResource(\"T_1\")\n\
         nodes/lo/node = SubResource(\"Anim_lo\")\n\
         nodes/hi/node = SubResource(\"Anim_hi\")\n\
         node_connections = [&\"output\", 0, &\"T\", &\"T\", 0, &\"lo\", &\"T\", 1, &\"hi\"]\n\n",
    );
    s.push_str(
        r#"[node name="Character" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
y = 0.0

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("Tree")
anim_player = NodePath("../AnimationPlayer")
active = true
"#,
    );
    s
}

/// Sets `parameters/<path>` entries on the AnimationTree every frame, which is
/// how a game drives these nodes — and the entry point the Wiring requirement
/// names.
fn script_src(sets: &[(&str, String)]) -> String {
    let body: String = sets
        .iter()
        .map(|(path, value)| format!("    t.set(\"parameters/{path}\", {value})\n"))
        .collect();
    // ⚠️ `pass` when there is nothing to set: a GDScript function with an
    // EMPTY body is a parse error, so the no-parameter case (the Transition's
    // default-input run) would fail in `from_source` long before any assertion.
    let body = if body.is_empty() {
        "    pass\n".to_string()
    } else {
        body
    };
    format!("extends Node2D\nfunc _process(delta):\n    var t = get_node(\"AnimationTree\")\n{body}")
}

fn num_prop(tree: &SceneTree, node: NodeId, prop: &str) -> f32 {
    match tree.get_node(node).expect("node exists").get_property(prop) {
        Variant::Float(f) => f as f32,
        Variant::Int(i) => i as f32,
        other => panic!("`{prop}` must be numeric, got {other:?}"),
    }
}

/// Loads `scene`, attaches a script setting `sets`, steps `frames`, and returns
/// the resulting `Sprite:y`.
fn run(scene_text: &str, sets: &[(&str, String)], frames: usize) -> f32 {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(scene_text).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let sprite = tree
        .find_child(scene_root, "Sprite", true)
        .expect("Sprite exists");
    let script = GDScriptNodeInstance::from_source(&script_src(sets), scene_root)
        .expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    for _ in 0..frames {
        ml.step(1.0 / 60.0);
    }
    num_prop(ml.tree(), sprite, "y")
}

/// **The acceptance test.** `TimeScale` and `Add2` both load from the scene and
/// both honour their runtime parameters.
#[test]
fn animation_tree_blend_tree_nodes_scene_load() {
    let scene = add2_timescale_scene(false);
    let base = [("TS/scale", "1.0".to_string()), ("Add/add_amount", "0.0".to_string())];

    // Six frames = 0.1s of a 1s ramp, so every reading below sits inside the
    // first loop and the comparison cannot be confounded by a wrap.
    let at_1x = run(&scene, &base, 6);
    assert!(
        at_1x > 1.0,
        "precondition: the tree must load and drive Sprite:y at all — got \
         {at_1x}. 0 is the old behaviour, where the whole AnimationTree came \
         back None because TimeScale had no loader arm"
    );

    // -- (a) TimeScale: double the scale, double the advance ----------------
    let at_2x = run(
        &scene,
        &[("TS/scale", "2.0".to_string()), ("Add/add_amount", "0.0".to_string())],
        6,
    );
    assert!(
        at_2x > at_1x * 1.5,
        "scale 2.0 must advance the ramp substantially further than scale 1.0: \
         1x gave y={at_1x}, 2x gave y={at_2x}. Equal values mean \
         `parameters/TS/scale` never reached the node — the arm loaded but the \
         parameter did not, or the scale default overwrote it"
    );

    // -- (b) Add2: the additive layer is added, and only when asked ---------
    let added = run(
        &scene,
        &[("TS/scale", "1.0".to_string()), ("Add/add_amount", "1.0".to_string())],
        6,
    );
    assert!(
        added > at_1x + 20.0,
        "add_amount 1.0 must add the `flat` layer (a constant 40) on top of the \
         ramp: amount 0 gave y={at_1x}, amount 1 gave y={added}. An unchanged \
         value means Add2 loaded as a pass-through or the parameter was dropped"
    );
}

/// 🔑 ONE unsupported node must no longer cost the whole tree.
#[test]
fn an_unsupported_node_no_longer_drops_the_whole_tree() {
    let sets = [("TS/scale", "1.0".to_string()), ("Add/add_amount", "0.0".to_string())];
    let intact = run(&add2_timescale_scene(false), &sets, 6);
    let with_bogus = run(&add2_timescale_scene(true), &sets, 6);

    assert!(
        with_bogus > 1.0,
        "a blend tree containing one node type the loader does not know must \
         STILL attach and animate; got y={with_bogus}. 0 means the unsupported \
         node's None propagated to the root and the project lost every \
         animation — the behaviour this bead replaces (and what \
         `unsupported_graph_node_builds_nothing` used to pin)"
    );
    // ...and it passes its input through rather than substituting something, so
    // the animation is the SAME as without it.
    assert!(
        (with_bogus - intact).abs() < 1.0,
        "the unsupported node sits on a single-input chain, so passing its input \
         through must leave the result unchanged: without it y={intact}, with it \
         y={with_bogus}"
    );
}

/// `transition_request` names an INPUT, which is Godot 4's spelling and was
/// read by nothing.
#[test]
fn transition_switches_by_input_name() {
    let scene = transition_scene();
    let on_a = run(&scene, &[], 4);
    assert!(
        (on_a - 10.0).abs() < 2.0,
        "with no request the Transition must play input 0 (`a`, a constant 10); \
         got y={on_a}"
    );

    let on_b = run(&scene, &[("T/transition_request", "\"b\"".to_string())], 4);
    assert!(
        (on_b - 90.0).abs() < 2.0,
        "`set(\"parameters/T/transition_request\", \"b\")` must switch to the \
         input NAMED b (a constant 90); got y={on_b}. A value still near 10 \
         means the by-name request was ignored — only the integer \
         `current_index` was ever read, and `transition_request` appeared \
         nowhere in the engine"
    );
}
