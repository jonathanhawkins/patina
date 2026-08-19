//! pat-s3rrv.45 acceptance: a 2D blend space blends its enclosing triangle by
//! barycentric weight, driven from a scene.
//!
//! Before this bead `sample_blend_space_2d` handled exactly one shape — four
//! points on an axis-aligned rectangle, bilinear — and fell back to
//! `nearest_point` for everything else, so the common 9-point directional
//! locomotion space **snapped between clips**. The `.tscn` builder read only
//! `blend_point_N/node` and `blend_point_N/pos`; `triangles`, `auto_triangles`,
//! `min_space`, `max_space`, `snap`, `blend_mode` and `sync` were all discarded.
//!
//! The scene here is Godot's own layout: nine points (centre + 8 directions),
//! each holding `Sprite:y` at a distinct constant so a blended pose is
//! arithmetically identifiable, plus the serialized `triangles`
//! `PackedInt32Array` an editor writes. A script sets
//! `parameters/blend_position` and real `MainLoop::step` frames do the rest —
//! no `BlendSpace2D` is built by hand in this file.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

/// One constant-valued clip. Time cannot influence the sampled value, so any
/// difference in `Sprite:y` is blending and nothing else.
fn clip(id: &str, name: &str, value: f32) -> String {
    format!(
        r#"[sub_resource type="Animation" id="Animation_{id}"]
resource_name = "{name}"
length = 1.0
loop_mode = 1
tracks/0/type = "value"
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite:y")
tracks/0/interp = 1
tracks/0/keys = {{"times": PackedFloat32Array(0, 1), "transitions": PackedFloat32Array(1, 1), "update": 0, "values": [{value:?}, {value:?}]}}

"#
    )
}

/// The nine points, in the index order the `triangles` list refers to:
/// 0 centre, then E, NE, N, NW, W, SW, S, SE. Each clip's constant is its index
/// times ten, so a barycentric mix lands on a value no single clip holds.
const POINTS: [(&str, f32, f32, f32); 9] = [
    ("c", 0.0, 0.0, 0.0),
    ("e", 1.0, 0.0, 10.0),
    ("ne", 1.0, 1.0, 20.0),
    ("n", 0.0, 1.0, 30.0),
    ("nw", -1.0, 1.0, 40.0),
    ("w", -1.0, 0.0, 50.0),
    ("sw", -1.0, -1.0, 60.0),
    ("s", 0.0, -1.0, 70.0),
    ("se", 1.0, -1.0, 80.0),
];

/// Godot's 8-direction triangulation: the four axis points fan off the centre,
/// the four corners fill the gaps. Written flat, exactly as Godot serializes it
/// — the parser has to read a `PackedInt32Array` of triples, not a list of
/// triples.
const TRIANGLES: &str = "0, 1, 3, 0, 3, 5, 0, 5, 7, 0, 7, 1, 1, 2, 3, 3, 4, 5, 5, 6, 7, 7, 8, 1";

/// Builds the scene. `blend_mode` is Godot's integer
/// (0 = INTERPOLATED, 1 = DISCRETE, 2 = CARRY).
fn scene_src(blend_mode: i64) -> String {
    let mut s = String::from("[gd_scene load_steps=24 format=3]\n\n");
    for (id, _, _, value) in POINTS {
        s.push_str(&clip(id, id, value));
    }

    let entries: Vec<String> = POINTS
        .iter()
        .map(|(id, _, _, _)| format!("\"{id}\": SubResource(\"Animation_{id}\")"))
        .collect();
    s.push_str(&format!(
        "[sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_main\"]\n_data = {{{}}}\n\n",
        entries.join(", ")
    ));

    for (id, _, _, _) in POINTS {
        s.push_str(&format!(
            "[sub_resource type=\"AnimationNodeAnimation\" id=\"Anim_{id}\"]\nanimation = &\"{id}\"\n\n"
        ));
    }

    s.push_str("[sub_resource type=\"AnimationNodeBlendSpace2D\" id=\"BlendSpace\"]\n");
    for (index, (id, x, y, _)) in POINTS.iter().enumerate() {
        s.push_str(&format!(
            "blend_point_{index}/node = SubResource(\"Anim_{id}\")\nblend_point_{index}/pos = Vector2({x:?}, {y:?})\n"
        ));
    }
    s.push_str(&format!(
        "min_space = Vector2(-1, -1)\nmax_space = Vector2(1, 1)\nsnap = Vector2(0, 0)\nblend_mode = {blend_mode}\ntriangles = PackedInt32Array({TRIANGLES})\n\n"
    ));

    s.push_str(
        r#"[node name="Character" type="Node2D"]

[node name="Sprite" type="Node2D" parent="."]
y = 0.0

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {"": SubResource("AnimationLibrary_main")}

[node name="AnimationTree" type="AnimationTree" parent="."]
tree_root = SubResource("BlendSpace")
anim_player = NodePath("../AnimationPlayer")
active = true
"#,
    );
    s
}

/// The script every case uses: it sets `parameters/blend_position` on the
/// AnimationTree, which is the only way a real game steers a blend space. The
/// blend position arrives as a `Vector2` built by the interpreter, not as a
/// value this test poked onto the graph.
fn script_src(x: f32, y: f32) -> String {
    format!(
        "extends Node2D\nfunc _process(delta):\n    var t = get_node(\"AnimationTree\")\n    t.set(\"parameters/blend_position\", Vector2({x:?}, {y:?}))\n"
    )
}

fn num_prop(tree: &SceneTree, node: NodeId, prop: &str) -> f64 {
    match tree.get_node(node).expect("node exists").get_property(prop) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{prop} is not numeric: {other:?}"),
    }
}

/// Boots the scene with the script attached and returns the sampled `Sprite:y`
/// after the blend position has been set and evaluated.
fn sampled_y(blend_mode: i64, x: f32, y: f32) -> f64 {
    let scene = PackedScene::from_tscn(&scene_src(blend_mode)).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let sprite = tree
        .get_node_or_null(scene_root, "Sprite")
        .expect("Sprite node");

    let script =
        GDScriptNodeInstance::from_source(&script_src(x, y), scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);
    let dt = 1.0 / 60.0;
    // Frame 1 runs `_process`, which sets the parameter; frame 2's animation
    // phase evaluates the graph with it.
    ml.step(dt);
    ml.step(dt);
    num_prop(ml.tree(), sprite, "y")
}

/// **The acceptance test.** `blend_position = Vector2(0.3, 0.3)` falls strictly
/// inside the triangle (centre, E, N), whose barycentric weights are
/// `0.4 / 0.3 / 0.3` — so the pose must be `0.4*0 + 0.3*10 + 0.3*30 = 12.0`.
///
/// 12.0 is a value **no point holds** (they are 0, 10, 20 … 80), which is what
/// makes this bite: the old nearest-point fallback answered 0.0 exactly, and a
/// two-way blend along an edge could not produce it either.
#[test]
fn blend_space_2d_barycentric_blend_from_scene() {
    let y = sampled_y(0, 0.3, 0.3);
    assert!(
        (y - 12.0).abs() < 1e-3,
        "barycentric mix of centre(0), E(10) and N(30) at (0.3, 0.3) is 12.0, got {y}"
    );
    // Not a single clip's value — the thing the pre-bead engine always returned.
    for (_, _, _, value) in POINTS {
        assert!(
            (y - value as f64).abs() > 1e-3,
            "the blended pose must not equal any single point's clip ({value})"
        );
    }
}

/// DISCRETE reverts to the single nearest point. At (0.3, 0.3) that is the
/// centre (distance 0.42) rather than E or N (0.76), so the pose is exactly
/// 0.0 — the same answer the old code gave for the wrong reason, which is why
/// the INTERPOLATED case above is the one that discriminates.
#[test]
fn blend_space_2d_discrete_mode_snaps_to_the_nearest_point() {
    let y = sampled_y(1, 0.3, 0.3);
    assert!(
        y.abs() < 1e-3,
        "DISCRETE must snap to the nearest point's clip (centre, 0.0), got {y}"
    );
}

/// A position outside the space clamps into it and keeps blending, rather than
/// snapping to an arbitrary nearest point. `(5.0, 0.25)` clamps to
/// `(1.0, 0.25)`, which lies inside the corner triangle (E, NE, N) with weights
/// `0.75 / 0.25 / 0` — so `0.75*10 + 0.25*20 = 12.5`.
#[test]
fn blend_space_2d_position_outside_the_space_clamps_and_still_blends() {
    let y = sampled_y(0, 5.0, 0.25);
    assert!(
        (y - 12.5).abs() < 1e-3,
        "an out-of-range position clamps to the hull edge and blends E(10)/NE(20) to 12.5, got {y}"
    );
}
