//! pat-s3rrv.119 — a `ParticleProcessMaterial`'s `scale_curve` and
//! `scale_min`/`scale_max` reach the emitter.
//!
//! `ParticleEmitter3D` already scales each particle by
//! `base_scale * scale_curve.sample(t)` (`particle3d.rs:544`), but nothing ever
//! set either term from a scene: `scale_curve`, `scale_min` and `scale_max` had
//! no producer, so every particle held a flat size of 1.0 for its whole life.
//!
//! 🔑 **Same two-hop shape as `color_ramp`.** Godot types `scale_curve` as
//! **Texture2D** (`set_param_texture`), so the editor assigns a `CurveTexture`
//! whose own `curve` property holds the `Curve` carrying `_data`. Reading
//! `_data` off the `scale_curve` reference itself finds nothing.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// The shape Godot writes. The curve runs 1.0 down to 0.25 — the same `_data`
/// the Line2D `width_curve` test already proves `bake_curve_samples` reads.
const WRAPPED: &str = r#"[gd_scene load_steps=4 format=3]

[sub_resource type="Curve" id="Curve_1"]
_data = [Vector2(0, 1), 0.0, 0.0, 0, 0, Vector2(1, 0.25), 0.0, 0.0, 0, 0]

[sub_resource type="CurveTexture" id="CurveTexture_1"]
curve = SubResource("Curve_1")

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
scale_min = 2.0
scale_max = 3.0
scale_curve = SubResource("CurveTexture_1")

[node name="Emitter" type="GPUParticles3D"]
process_material = SubResource("ParticleProcessMaterial_1")
"#;

/// A hand-written scene pointing `scale_curve` straight at the Curve, with no
/// CurveTexture wrapper.
const DIRECT: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Curve" id="Curve_1"]
_data = [Vector2(0, 1), 0.0, 0.0, 0, 0, Vector2(1, 0.25), 0.0, 0.0, 0, 0]

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
scale_curve = SubResource("Curve_1")

[node name="Emitter" type="GPUParticles3D"]
process_material = SubResource("ParticleProcessMaterial_1")
"#;

/// No curve authored — the path every existing particle scene uses.
const FLAT: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
gravity = Vector3(0, -10, 0)

[node name="Emitter" type="GPUParticles3D"]
process_material = SubResource("ParticleProcessMaterial_1")
"#;

fn emitter_prop(tscn: &str, key: &str) -> Variant {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let node_id = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree.get_node(node_id)
        .expect("emitter exists")
        .get_property(key)
}

fn samples(tscn: &str, label: &str) -> Vec<f32> {
    match emitter_prop(tscn, "__scale_curve_samples") {
        Variant::PackedFloat32Array(v) => v,
        other => panic!("{label}: expected baked scale-curve samples, got {other:?}"),
    }
}

/// **The core of the bead's clause**: the two-hop Godot shape resolves, and the
/// baked curve really descends rather than staying flat.
#[test]
fn a_curve_texture_wrapped_scale_curve_reaches_the_node() {
    let s = samples(WRAPPED, "wrapped");
    assert!(s.len() >= 2, "a baked curve needs at least two samples");

    // ⚠️ Asserting only "samples exist" would pass for a curve baked as all
    // 1.0 — which is exactly what the emitter already defaulted to. The ends
    // are what prove the authored shape survived.
    assert!(
        (s[0] - 1.0).abs() < 1e-3,
        "the curve starts at 1.0, got {}",
        s[0]
    );
    assert!(
        (s[s.len() - 1] - 0.25).abs() < 1e-3,
        "the curve ends at 0.25, got {}",
        s[s.len() - 1]
    );
    assert!(
        s[0] > s[s.len() - 1],
        "a flat bake would make these equal and the particle would never shrink"
    );
}

/// ⚠️ The wrapper is not mandatory; following only `scale_curve -> curve` would
/// leave this scene with no curve and no error.
#[test]
fn a_scale_curve_pointing_straight_at_a_curve_also_resolves() {
    let s = samples(DIRECT, "direct");
    assert!((s[0] - 1.0).abs() < 1e-3);
    assert!((s[s.len() - 1] - 0.25).abs() < 1e-3);
}

/// `scale_min`/`scale_max` are plain floats on the material and were not in the
/// copied key list at all, so a scene sizing its particles had no effect.
#[test]
fn the_scale_range_reaches_the_node() {
    assert!(
        matches!(emitter_prop(WRAPPED, "scale_min"), Variant::Float(f) if (f - 2.0).abs() < 1e-6),
        "scale_min must flatten onto the node"
    );
    assert!(
        matches!(emitter_prop(WRAPPED, "scale_max"), Variant::Float(f) if (f - 3.0).abs() < 1e-6),
        "scale_max must flatten onto the node"
    );
}

/// The negative control. A flattener that wrote the companion unconditionally —
/// or wrote an empty one — would pass the tests above while overriding the
/// emitter default for every particle scene in the project.
#[test]
fn a_material_without_a_scale_curve_gets_no_companion() {
    assert_eq!(
        emitter_prop(FLAT, "__scale_curve_samples"),
        Variant::Nil,
        "no curve authored, so no companion may appear"
    );
    // And the material's other keys still flatten, so the scene is otherwise
    // parsed normally rather than skipped.
    assert!(
        matches!(emitter_prop(FLAT, "gravity"), Variant::Vector3(g) if g.y < 0.0),
        "the rest of the material must still reach the node"
    );
}
