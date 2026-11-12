//! pat-s3rrv.206 — TEMPORARY DIAGNOSTIC. Prints what the probe actually puts
//! on screen; asserts nothing.
//!
//! ⚠️ **This file is scaffolding and should be deleted once it has answered its
//! question.** It exists because `a_rough_surface_takes_less_reflection_than_a_mirror`
//! reports `smooth == 0` under a strict red predicate while the acceptance
//! passes under a loose one, and those two readings have very different causes:
//!
//! * the reflection is present but desaturated → a threshold / energy-split
//!   problem in shading;
//! * the reflection is absent and the loose predicate is being satisfied by the
//!   probe's own orange `BOX_COLOR_3D` gizmo → the capture never sees the wall,
//!   which is a different problem than the bead's title implies.
//!
//! Counting pixels cannot separate those. Printing their colours can.
//!
//! Deliberately always-green: a knowingly-red test in a shared crate fails
//! every peer's verify. Run the built binary with `--nocapture` to read it.
//!
//! The fixture is copied from `reflection_probe_capture_render.rs` rather than
//! shared, because that file was being edited by another agent.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, render_editor_3d_frame};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 128;
const H: u32 = 128;

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
transform = Transform3D(1, 0, 0, 0, -1, 0, 0, 0, -1, -2, 2, -2)
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

/// How the post chain is configured for a census run.
#[derive(Clone, Copy, PartialEq)]
enum Post {
    /// The editor preview default: filmic tonemap + glow.
    Default,
    /// Glow off. Glow is a separable gaussian (radius 4) that bleeds BRIGHT
    /// neighbours into a pixel, and the preview sky is a bright blue-grey
    /// `(0.385, 0.454, 0.55)`. Blooming that into a red mirror pixel raises g
    /// and b relative to r, which is exactly what `g < r * 0.35` measures — so
    /// a correct reflection could fail the ratio purely from post-processing.
    NoGlow,
    /// Glow off and tonemap linear, i.e. as close to the raw shaded buffer as
    /// this entry point allows.
    Raw,
}

fn render_with(src: &str, post: Post) -> Vec<Color> {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;
    if post != Post::Default {
        vp.environment.environment.glow_enabled = false;
    }
    if post == Post::Raw {
        vp.environment.environment.tone_mapper = gdserver3d::environment::ToneMapper::Linear;
    }
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

fn captured_wall_red(c: Color) -> bool {
    c.r > 0.05 && c.g < c.r * 0.35 && c.b < c.r * 0.35
}

/// The probe's own gizmo colour, which `red_dominant` cannot reject.
const BOX_COLOR_3D: (f32, f32, f32) = (0.90, 0.62, 0.20);

fn near_gizmo(c: Color) -> bool {
    (c.r - BOX_COLOR_3D.0).abs() < 0.15
        && (c.g - BOX_COLOR_3D.1).abs() < 0.15
        && (c.b - BOX_COLOR_3D.2).abs() < 0.15
}

/// Prints a census of every pixel the probe changes, for one roughness.
fn census(label: &str, roughness: f32, post: Post) {
    let with = render_with(&scene_src(PROBE_PLAIN, roughness), post);
    let without = render_with(&scene_src("", roughness), post);
    let idx = differing(&with, &without);

    let loose = idx.iter().filter(|&&i| red_dominant(with[i])).count();
    let strict = idx.iter().filter(|&&i| captured_wall_red(with[i])).count();
    let gizmo = idx.iter().filter(|&&i| near_gizmo(with[i])).count();

    println!("\n=== {label} (roughness = {roughness}) ===");
    println!("  differing pixels ....... {}", idx.len());
    println!("  red_dominant (loose) ... {loose}");
    println!("  captured_wall_red ...... {strict}");
    println!("  near BOX_COLOR_3D gizmo. {gizmo}");

    // The colour histogram is the actual evidence: it says whether the loose
    // count is orange gizmo or genuine red, and whether anything reflective is
    // reaching the mirror at all.
    let mut buckets: std::collections::HashMap<(i32, i32, i32), usize> =
        std::collections::HashMap::new();
    for &i in &idx {
        let c = with[i];
        let key = (
            (c.r * 20.0).round() as i32,
            (c.g * 20.0).round() as i32,
            (c.b * 20.0).round() as i32,
        );
        *buckets.entry(key).or_insert(0) += 1;
    }
    let mut rows: Vec<_> = buckets.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    println!("  distinct colours (top 14, value = bucket/20):");
    for ((r, g, b), n) in rows.iter().take(14) {
        let (rf, gf, bf) = (*r as f32 / 20.0, *g as f32 / 20.0, *b as f32 / 20.0);
        let tag = if captured_wall_red(Color::new(rf, gf, bf, 1.0)) {
            "WALL-RED"
        } else if near_gizmo(Color::new(rf, gf, bf, 1.0)) {
            "gizmo"
        } else if red_dominant(Color::new(rf, gf, bf, 1.0)) {
            "reddish"
        } else {
            ""
        };
        println!("    {n:6} x ({rf:.2}, {gf:.2}, {bf:.2})  {tag}");
    }
}

/// 🔑 Does the probe node reach the RENDER ITEM LIST — i.e. is a gizmo drawn?
///
/// This settles a question two of us could not settle by reading, and which the
/// whole `captured_wall_red` rationale rests on. `Editor3DRenderInputs::items`
/// is documented as "meshes **+ primitive markers**", and `primitive_for_class`
/// sends any non-Camera/non-Light class to `PrimitiveShape::Box`, drawn as a
/// `Mesh3D::cube(0.4)` in orange `BOX_COLOR_3D`.
///
/// So:
///
/// * `items` grows by 1 when the probe is added → the probe IS drawn as a
///   marker, the gizmo contaminates `differing`, and the strict predicate is
///   justified;
/// * `items` unchanged → the probe is skipped (`collect_scene_content` gates on
///   `is_class("Node3D")`, which consults a ClassDB this binary never
///   populates), **no marker exists**, and the gizmo rationale is false — the
///   changed pixels are something else, most likely the reflection itself.
///
/// Reported alongside `env_effects.reflection_probes`, which proves the probe
/// was found as an env effect either way — `is_class` short-circuits on an exact
/// name match, so probe discovery does not depend on ClassDB the way the
/// `Node3D` gate does.
fn item_census(label: &str, roughness: f32) {
    let mut vp = Viewport3D::default();
    vp.width = W;
    vp.height = H;

    let with = editor_3d_render_inputs(&vp, &tree(&scene_src(PROBE_PLAIN, roughness)));
    let without = editor_3d_render_inputs(&vp, &tree(&scene_src("", roughness)));

    println!("\n--- render items, {label} ---");
    println!(
        "  items with probe ....... {} (probes found: {})",
        with.items.len(),
        with.env_effects.reflection_probes.len()
    );
    println!(
        "  items without probe .... {} (probes found: {})",
        without.items.len(),
        without.env_effects.reflection_probes.len()
    );
    println!(
        "  => probe {} the item list, so a gizmo IS {}drawn",
        if with.items.len() > without.items.len() {
            "ENTERS"
        } else {
            "is ABSENT from"
        },
        if with.items.len() > without.items.len() {
            ""
        } else {
            "NOT "
        }
    );
}

/// Always green. Run the binary with `--nocapture` to read the census.
///
/// Reading the output:
///
/// * `captured_wall_red > 0` under `Default` → the reflection is there and the
///   assertion should already pass; look for a fixture difference instead.
/// * `captured_wall_red == 0` under `Default` but `> 0` under `NoGlow`/`Raw` →
///   the reflection IS reaching the mirror and the post chain is desaturating
///   it. The predicate is then being applied at the wrong stage, not measuring
///   the wrong thing.
/// * `captured_wall_red == 0` in ALL THREE, with the loose count sitting near
///   the gizmo count → nothing is reflecting and the acceptance is green on the
///   probe's own marker.
#[test]
fn probe_mirror_pixel_census() {
    // Run this FIRST: if the probe never reaches the item list, the gizmo
    // cannot be contaminating any count below and the colour census should be
    // read as measuring the reflection alone.
    item_census("SMOOTH mirror", 0.0);
    item_census("ROUGH mirror", 1.0);

    for (name, post) in [
        ("post = editor default (filmic + glow)", Post::Default),
        ("post = glow OFF", Post::NoGlow),
        ("post = glow OFF + linear tonemap", Post::Raw),
    ] {
        println!("\n########## {name} ##########");
        census("SMOOTH mirror", 0.0, post);
        census("ROUGH mirror", 1.0, post);
    }
}
