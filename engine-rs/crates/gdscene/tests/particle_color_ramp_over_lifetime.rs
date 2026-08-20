//! pat-s3rrv.116 — a `ParticleProcessMaterial`'s `color_ramp` reaches the node.
//!
//! `MainLoop`'s emitter setup collapsed every particle system to
//! `ColorCurve::linear(color, color)` — the degenerate one-colour gradient —
//! because nothing ever parsed the ramp. A Godot scene authoring a red-to-blue
//! fade emitted flat white particles.
//!
//! 🔑 **The stops are two hops from the material.** Godot types `color_ramp` as
//! **Texture2D**, not Gradient (`ParticleProcessMaterial.xml`): the editor
//! assigns a `GradientTexture1D`, and the `offsets`/`colors` arrays live on
//! *its* `gradient` (`GradientTexture1D.xml`). Looking for `offsets` on the ramp
//! itself finds nothing at all, which is the shape of the bug this pins.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// The shape Godot writes: material → GradientTexture1D → Gradient.
const WRAPPED: &str = r#"[gd_scene load_steps=4 format=3]

[sub_resource type="Gradient" id="Gradient_1"]
offsets = PackedFloat32Array(0, 1)
colors = PackedColorArray(1, 0, 0, 1, 0, 0, 1, 1)

[sub_resource type="GradientTexture1D" id="GradientTexture1D_1"]
gradient = SubResource("Gradient_1")

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
color_ramp = SubResource("GradientTexture1D_1")

[node name="Emitter" type="GPUParticles3D"]
process_material = SubResource("ParticleProcessMaterial_1")
"#;

/// A hand-written scene that skips the texture wrapper and points `color_ramp`
/// straight at a Gradient. Godot's own editor does not write this, but a
/// hand-authored or generated scene can, and resolving only the wrapper would
/// drop it silently.
const DIRECT: &str = r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Gradient" id="Gradient_1"]
offsets = PackedFloat32Array(0, 1)
colors = PackedColorArray(1, 0, 0, 1, 0, 0, 1, 1)

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
color_ramp = SubResource("Gradient_1")

[node name="Emitter" type="GPUParticles3D"]
process_material = SubResource("ParticleProcessMaterial_1")
"#;

/// No ramp at all — the pre-existing flat-`color` path, which must keep working.
const FLAT: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ParticleProcessMaterial" id="ParticleProcessMaterial_1"]
color = Color(0, 1, 0, 1)

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

/// Both stop arrays must land on the node, because by emitter-setup time the
/// `PackedScene` is gone and only the `SceneTree` remains.
fn assert_red_to_blue(tscn: &str, label: &str) {
    match emitter_prop(tscn, "__color_ramp_offsets") {
        Variant::PackedFloat32Array(v) => {
            assert_eq!(v, vec![0.0, 1.0], "{label}: offsets");
        }
        other => panic!("{label}: expected the ramp offsets on the node, got {other:?}"),
    }
    match emitter_prop(tscn, "__color_ramp_colors") {
        Variant::PackedColorArray(v) => {
            assert_eq!(v.len(), 2, "{label}: two stops");
            assert_eq!((v[0].r, v[0].b), (1.0, 0.0), "{label}: first stop is red");
            assert_eq!((v[1].r, v[1].b), (0.0, 1.0), "{label}: last stop is blue");
        }
        other => panic!("{label}: expected the ramp colors on the node, got {other:?}"),
    }
}

/// **The core of the bead**: the Godot-authored two-hop shape resolves.
#[test]
fn a_gradient_texture_wrapped_ramp_reaches_the_node() {
    assert_red_to_blue(WRAPPED, "wrapped");
}

/// ⚠️ The wrapper is not mandatory. Following only `color_ramp -> gradient`
/// would leave this scene with no ramp and no error.
#[test]
fn a_ramp_pointing_straight_at_a_gradient_also_resolves() {
    assert_red_to_blue(DIRECT, "direct");
}

/// The negative control. Without it, a flattener that wrote the companions
/// unconditionally — or wrote empty ones — would pass the tests above while
/// breaking every particle scene that authors a flat colour.
#[test]
fn a_material_without_a_ramp_gets_no_companions() {
    assert_eq!(
        emitter_prop(FLAT, "__color_ramp_offsets"),
        Variant::Nil,
        "no ramp authored, so no companion may appear"
    );
    assert_eq!(emitter_prop(FLAT, "__color_ramp_colors"), Variant::Nil);
    // And the flat `color` still flattens, so the emitter keeps its old path.
    assert!(
        matches!(emitter_prop(FLAT, "color"), Variant::Color(c) if c.g == 1.0),
        "the material's flat colour must still reach the node"
    );
}
