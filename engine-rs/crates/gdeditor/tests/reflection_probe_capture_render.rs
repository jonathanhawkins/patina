//! pat-s3rrv.206 — a `ReflectionProbe` captures a real cubemap of its
//! surroundings, and a mirror-like surface inside it reflects that capture.
//!
//! Godot ground truth: `upstream/godot/scene/3d/reflection_probe.cpp`.
//!
//! # What was broken
//!
//! A probe contributed a flat `ambient_color` term and nothing else
//! (`sample_probes_at`), so it could never mirror geometry — the term carries
//! no direction at all. Two consequences the bead calls out, both fixed here:
//!
//! 1. `gdserver3d/src/box_projection.rs` — 185 lines of complete,
//!    unit-tested parallax-correction math — had **zero callers**. Authoring
//!    `box_projection = true` in a `.tscn` changed nothing whatsoever.
//! 2. `ReflectionProbeUpdateMode` was parsed off the node and then never
//!    branched on, so `Once` and `Always` were the same thing.
//!
//! # How these assertions are shaped
//!
//! ⚠️ Everything at the frame level is asserted as a **difference between two
//! renders** (probe vs no probe), never as absolute pixel positions. The red
//! wall is visible directly in frame as well as by reflection; comparing two
//! frames of the same scene cancels the direct view exactly, so what is left is
//! the probe's contribution and nothing else.
//!
//! ⚠️ And by **hue dominance**, not exact colour: the fog, SSAO and tonemap
//! passes all run after shading and are depth-dependent, so an exact-value
//! assertion would be measuring the post chain. None of them turns a red pixel
//! green.
//!
//! ⚠️ NOT asserted here: that the reflection lands at particular screen
//! coordinates. `render_editor_3d_frame` renders from the **`Viewport3D` orbit
//! camera** (yaw 45°, pitch −28.6°), not from any `Camera3D` in the `.tscn`, so
//! world X and Z both project mostly horizontally and screen axes do not
//! correspond to world axes. A sibling bead learned that the hard way.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver3d::reflection_probe::{
    ProbeReflection, ReflectionCubemap, ReflectionProbe, ReflectionProbeId,
    ReflectionProbeUpdateMode,
};
use std::sync::Arc;

const W: u32 = 128;
const H: u32 = 128;

// ---------------------------------------------------------------------------
// Scene fixtures
// ---------------------------------------------------------------------------

/// A mirror plane at the origin with a red wall high on the far side.
///
/// ⚠️ The material goes on `surface_material_override/0`, NOT on a bare
/// `material` key. `MeshInstance3D` has no `material` member in Godot, and
/// `packed_scene`'s `__material__*` flattening resolves the id from the
/// override (or from the mesh sub-resource), so a bare `material =` authors a
/// property the engine is right to ignore — the mesh then reaches the renderer
/// with no material at all and the symptom is "0 pixels differed".
///
/// The wall sits at `(-2, 2, -2)`: high, and on the opposite side from the
/// editor camera (which orbits to +X/+Z), so it is in the mirror's reflected
/// hemisphere without standing between the camera and the mirror.
fn scene_src(probe: &str, mirror_roughness: f32) -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="StandardMaterial3D" id="Mirror_1"]
albedo_color = Color(1, 1, 1, 1)
metallic = 1.0
roughness = {mirror_roughness}

[sub_resource type="StandardMaterial3D" id="Wall_1"]
albedo_color = Color(1, 0, 0, 1)
roughness = 1.0

[node name="Root" type="Node3D"]

[node name="Mirror" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
surface_material_override/0 = SubResource("Mirror_1")

[node name="Wall" type="MeshInstance3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, -2, 2, -2)
mesh = "primitive:PlaneMesh"
surface_material_override/0 = SubResource("Wall_1")

{probe}
"#
    )
}

const PROBE_PLAIN: &str = r#"[node name="Probe" type="ReflectionProbe" parent="."]
size = Vector3(20, 20, 20)
"#;

fn tree(src: &str) -> SceneTree {
    let packed = PackedScene::from_tscn(src).expect("parse tscn");
    let mut t = SceneTree::new();
    let root = t.root_id();
    add_packed_scene_to_tree(&mut t, root, &packed).expect("instance scene");
    t
}

fn render(src: &str) -> Vec<Color> {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;
    render_editor_3d_frame(&vp, &tree(src)).pixels
}

fn differing(a: &[Color], b: &[Color]) -> Vec<usize> {
    assert_eq!(a.len(), b.len(), "frames are the same size");
    (0..a.len())
        .filter(|&i| {
            (a[i].r - b[i].r).abs() > 1e-4
                || (a[i].g - b[i].g).abs() > 1e-4
                || (a[i].b - b[i].b).abs() > 1e-4
        })
        .collect()
}

fn red_dominant(c: Color) -> bool {
    c.r > c.g + 0.02 && c.r > c.b + 0.02
}

// ---------------------------------------------------------------------------
// The acceptance
// ---------------------------------------------------------------------------

/// 🔑 **THE ACCEPTANCE.** A mirror-like plane inside a `ReflectionProbe`, with a
/// red wall in its reflected hemisphere, renders red reflection pixels that are
/// absent from the identical scene with no probe.
#[test]
fn reflection_probe_capture_render() {
    let with_probe = render(&scene_src(PROBE_PLAIN, 0.0));
    let without = render(&scene_src("", 0.0));

    let changed = differing(&with_probe, &without);
    assert!(
        !changed.is_empty(),
        "a probe over a mirror must change pixels. Before this change a probe \
         contributed only a flat `ambient_color` — with Godot's default BLACK \
         ambient that is exactly zero, so an authored probe could not alter one \
         pixel however reflective the surface was"
    );

    let reds = changed
        .iter()
        .filter(|&&i| red_dominant(with_probe[i]))
        .count();
    assert!(
        reds > 0,
        "the probe's contribution must be the RED wall it captured, not merely \
         'some change': {} pixels differed but none were red-dominant. A flat \
         ambient term would tint uniformly and fail here",
        changed.len()
    );

    // The probe must not repaint the whole frame — it applies to the reflective
    // surface inside its box, not to the backdrop.
    assert!(
        changed.len() < with_probe.len(),
        "the probe changed every pixel in the frame, which means it is acting \
         as a global tint rather than a reflection"
    );
}

/// ⚠️ The control. Without it the acceptance would also pass on a build that
/// added the captured colour to *every* surface regardless of how reflective it
/// is — which would wash out every rough material in a scene.
///
/// A fully rough mirror has no coherent reflected direction, so it must take
/// far less of the capture than a smooth one.
#[test]
fn a_rough_surface_takes_less_reflection_than_a_mirror() {
    let baseline_smooth = render(&scene_src("", 0.0));
    let baseline_rough = render(&scene_src("", 1.0));

    let smooth = differing(&render(&scene_src(PROBE_PLAIN, 0.0)), &baseline_smooth).len();
    let rough = differing(&render(&scene_src(PROBE_PLAIN, 1.0)), &baseline_rough).len();

    assert!(
        smooth > 0,
        "the smooth mirror must take the reflection (guards against this test \
         passing because NOTHING reflects)"
    );
    assert!(
        rough < smooth,
        "a roughness = 1.0 surface must take less of the probe capture than a \
         roughness = 0.0 mirror; got rough={rough} vs smooth={smooth}"
    );
}

// ---------------------------------------------------------------------------
// box_projection — the module that had zero callers
// ---------------------------------------------------------------------------

/// 🔑 `box_projection` changes the direction the cubemap is sampled along.
///
/// Asserted directly on [`ProbeReflection::sample`] rather than through a
/// frame, because this is a statement about the *lookup*, and a frame-level
/// assertion would depend on the whole shading chain agreeing first.
///
/// ⚠️ The fixture had to be built around two coincidence points that make the
/// correction invisible:
///
/// 1. **A single-texel face cannot show it.** With a box centred on the capture
///    origin, a ray exiting the `+X` wall lands at `x = 5`, which dominates
///    `|y|` and `|z|` for any point on that wall — so the corrected ray always
///    selects the *same face* as the raw one. The correction moves the **UV
///    within** the face, so the face needs more than one texel.
/// 2. **A ray from the origin cannot show it either** — there the exit point
///    and the direction agree and the correction is the identity.
///
/// So: a 2x2 `+X` face, and a surface point lifted off the origin. The raw ray
/// `(1,0,0)` samples the face centre; the corrected ray from `(0,4,0)` exits at
/// `(5,4,0)` and samples a texel one row up. A build that never calls
/// `box_project_reflection` returns the same colour for both and fails here.
#[test]
fn box_projection_changes_the_sampled_direction() {
    use gdcore::math::Vector3;
    use gdserver3d::reflection_probe::CubeFace;

    const RAW: Color = Color::new(0.0, 1.0, 0.0, 1.0);
    const CORRECTED: Color = Color::new(1.0, 0.0, 0.0, 1.0);
    let black = Color::new(0.0, 0.0, 0.0, 1.0);

    // Row-major 2x2: index 3 is the texel the uncorrected ray hits (u=v=0.5),
    // index 1 the one the corrected ray hits (u=0.5, v=0.1).
    let mut cube = ReflectionCubemap::new(2, black);
    cube.set_face(CubeFace::PosX, vec![black, CORRECTED, black, RAW]);
    let cube = Arc::new(cube);

    let world_pos = Vector3::new(0.0, 4.0, 0.0);
    let reflection = Vector3::new(1.0, 0.0, 0.0);
    let base = ProbeReflection {
        cubemap: cube,
        box_min: Vector3::new(-5.0, -5.0, -5.0),
        box_max: Vector3::new(5.0, 5.0, 5.0),
        origin: Vector3::ZERO,
        box_projection: false,
        intensity: 1.0,
    };
    let projected = ProbeReflection {
        box_projection: true,
        ..base.clone()
    };

    let plain = base.sample(world_pos, reflection);
    let corrected = projected.sample(world_pos, reflection);

    assert!(
        (plain.g - RAW.g).abs() < 1e-6 && plain.r < 0.5,
        "without box projection the raw reflection direction is the lookup, \
         which hits the face centre: expected {RAW:?}, got {plain:?}"
    );
    assert!(
        (corrected.r - CORRECTED.r).abs() < 1e-6 && corrected.g < 0.5,
        "with box_projection the ray must be re-aimed at where it LEAVES THE \
         BOX, selecting a different texel: expected {CORRECTED:?}, got \
         {corrected:?}. Identical results mean `box_project_reflection` is not \
         being called — the exact state this bead exists to fix"
    );
}

// ---------------------------------------------------------------------------
// update_mode — parsed, then never branched on
// ---------------------------------------------------------------------------

/// 🔑 `update_mode` decides whether a probe re-captures.
///
/// `Once` captures on the first pass and then **keeps** that cubemap while the
/// scene changes around it; `Always` re-captures every frame. Before this
/// change the enum was parsed off the `.tscn` and never read, so the two were
/// indistinguishable.
#[test]
fn update_mode_gates_recapture() {
    let mut probe = ReflectionProbe::new(ReflectionProbeId(1));

    // Godot's default is ONCE, and an uncaptured probe always needs a capture.
    assert_eq!(probe.update_mode, ReflectionProbeUpdateMode::Once);
    assert!(
        probe.needs_capture(),
        "a probe that has never been captured needs a capture whatever its mode"
    );

    probe.captured = Some(Arc::new(ReflectionCubemap::new(
        2,
        Color::new(1.0, 0.0, 0.0, 1.0),
    )));
    assert!(
        !probe.needs_capture(),
        "ONCE must KEEP its capture — this is the half that makes a moved wall \
         stay in the old reflection"
    );

    probe.update_mode = ReflectionProbeUpdateMode::Always;
    assert!(
        probe.needs_capture(),
        "ALWAYS must re-capture even though a cubemap is already present"
    );
}

/// The capture origin honours `origin_offset`, which is what the six face
/// renders are centred on.
#[test]
fn the_capture_origin_follows_origin_offset() {
    use gdcore::math::Vector3;

    let mut probe = ReflectionProbe::new(ReflectionProbeId(2));
    probe.origin_offset = Vector3::new(1.0, 2.0, 3.0);
    let o = probe.capture_origin();
    assert_eq!((o.x, o.y, o.z), (1.0, 2.0, 3.0));

    // ⚠️ And the influence box moves with it — `world_aabb_min_max` offsets the
    // centre too, so a probe with an offset does not capture from a point
    // outside the box it lights.
    let (min, max) = probe.world_aabb_min_max();
    assert!(
        o.x >= min.x && o.x <= max.x && o.y >= min.y && o.y <= max.y,
        "the capture origin must lie inside the probe's own influence box"
    );
}
