//! pat-0q2kx.195 (partial) — a `ReflectionProbe`'s ambient reaches the shaded
//! frame, and its `ambient_mode` / a `Decal`'s texture channels survive the
//! `.tscn`.
//!
//! Godot ground truth: `upstream/godot/scene/3d/reflection_probe.cpp` and
//! `decal.cpp`; defaults from `doc/classes/{ReflectionProbe,Decal}.xml`.
//!
//! ⚠️ **The bead's premise is substantially stale — the tracker should be
//! corrected.** It says the only feeder for these nodes is the test-only
//! `RenderServer3DAdapter` and that decals/occluders have no producer at all.
//! That was true when the audit ran; pat-0q2kx.132 has since landed
//! `scene_env_effects` in `viewport_3d_render.rs`, which walks the tree for
//! `FogVolume` / `Decal` / `ReflectionProbe` / `OccluderInstance3D`, snapshots
//! them with their global transforms, and hands them to both the editor and
//! Play render paths — with its own acceptance test
//! (`envfx_nodes_render_from_scene.rs`) covering the decal, fog and occluder
//! clauses. Item (2) of this bead's implement list was already done.
//!
//! What was genuinely still broken, and is what this file covers:
//!
//! 1. **The probe was wired but inert.** `sample_reflection_probes` had ZERO
//!    production callers — neither shading loop ever read the probe list, so a
//!    probe could be parsed, transformed, stored and handed to the renderer and
//!    still change not one pixel. That is exactly the bead's "prove it" clause,
//!    and it failed before this change.
//! 2. **`ambient_mode` was never extracted**, and it gates the probe's entire
//!    contribution in `sample_probes_at` — so `ambient_mode = 0` (Disabled) in a
//!    `.tscn` was silently ignored and the probe lit anyway.
//! 3. **A `Decal`'s four texture channels were never read off the node**, so a
//!    scene naming `texture_albedo` reached the renderer with `None` in every
//!    channel.
//! 4. **`LightmapProbe` was not registered at all**, and `Decal` /
//!    `ReflectionProbe` / `VoxelGI` / `LightmapGI` were registered bare.
//!
//! ⚠️ NOT delivered: sampling the projected decal *textures* during shading
//! (the paths are carried, but no pass reads the pixels), per-fragment probe
//! sampling, box projection, and VoxelGI/LightmapGI baking. See the completion
//! report.
//!
//! NOTE ON THE FILE NAME: the acceptance names
//! `reflection_probe_decal_scene_render`; with `-p gdeditor --test` that
//! command is 71 characters and the verifier truncates at 58, so the binary is
//! `probe_decal` (command = 48) and the acceptance name is the test function.

use gdcore::math::Color;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{editor_3d_render_inputs, render_editor_3d_frame};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver3d::reflection_probe::ReflectionProbeAmbientMode;

fn load(tscn: &str) -> SceneTree {
    // ⚠️ The render list filters on `is_class("Node3D")`, which reads the
    // ClassDB. Without this only the literal `Node3D` root is collected — as an
    // orange editor marker — and the `MeshInstance3D` floor every assertion in
    // this file is about never reaches the renderer: each test was measuring
    // the root's marker cube instead of the floor.
    gdobject::class_db::register_3d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

fn viewport() -> Viewport3D {
    let mut vp = Viewport3D::default();
    vp.width = 96;
    vp.height = 96;
    // ⚠️ The preview sun is off deliberately. At its default energy of 1.0 a
    // white floor clamps to 1.0 in every channel, and a probe that then adds
    // red would be invisible — `r` is already saturated, so the frame would
    // come back byte-identical and this file would report "the probe does
    // nothing" when the probe works fine. Disabling it leaves the 0.18 fill
    // light (`viewport_3d_render.rs:1752`), so geometry still shades and the
    // probe's contribution has headroom to show up.
    vp.environment.sun_enabled = false;
    vp
}

/// A floor plane in view, with no probe — the control.
const NO_PROBE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"
"#;

/// The same scene wrapped in a probe whose interior ambient is red.
///
/// `ambient_mode = 2` is `CONSTANT_COLOR`; the box is big enough to contain the
/// floor, which is what `contains_point` tests.
const RED_PROBE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Probe" type="ReflectionProbe" parent="."]
size = Vector3(20, 20, 20)
interior = true
ambient_mode = 2
ambient_color = Color(1, 0, 0, 1)
ambient_color_energy = 1.0
"#;

/// [`RED_PROBE`] with its ambient switched off and nothing else changed — the
/// control for the red-ambient acceptance.
///
/// ⚠️ The control must hold a probe, not omit one. A probe contributes two
/// things: the flat `ambient_color` this file is about, and the REFLECTION of
/// its captured cubemap (pat-s3rrv.206), which Godot applies to any surface
/// that is not fully rough — the mesh here has no material, and Godot's default
/// material is `ROUGHNESS = 0.8` (`scene_shader_forward_clustered.cpp:920`). A
/// ReflectionProbe is also drawn as an editor marker. Comparing against a
/// probe-less scene therefore measured the reflection and the marker as well,
/// which change green and blue. Holding the probe fixed isolates the ambient.
const RED_PROBE_AMBIENT_OFF: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Probe" type="ReflectionProbe" parent="."]
size = Vector3(20, 20, 20)
interior = true
ambient_mode = 0
ambient_color = Color(1, 0, 0, 1)
ambient_color_energy = 1.0
"#;

/// A probe at Godot's defaults: `ambient_mode` 1 (`ENVIRONMENT`,
/// `ReflectionProbe.xml:23`) and a black `ambient_color`.
const DEFAULT_PROBE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Probe" type="ReflectionProbe" parent="."]
size = Vector3(20, 20, 20)
"#;

/// Same probe, but explicitly disabled. `ambient_mode = 0` is `DISABLED`.
const DISABLED_PROBE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Probe" type="ReflectionProbe" parent="."]
size = Vector3(20, 20, 20)
ambient_mode = 0
ambient_color = Color(1, 0, 0, 1)
"#;

/// A decal with all four texture channels named, to check they survive the
/// `.tscn` → node → snapshot path.
const TEXTURED_DECAL: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 6)
current = true

[node name="Floor" type="MeshInstance3D" parent="."]
mesh = "primitive:PlaneMesh"

[node name="Splat" type="Decal" parent="."]
size = Vector3(3, 3, 3)
modulate = Color(0, 0, 1, 1)
albedo_mix = 1.0
texture_albedo = "res://splat_albedo.png"
texture_normal = "res://splat_normal.png"
texture_orm = "res://splat_orm.png"
texture_emission = "res://splat_emission.png"
"#;

/// How many pixels differ between two frames of the same size.
fn differing_pixels(a: &[Color], b: &[Color]) -> usize {
    a.iter()
        .zip(b.iter())
        .filter(|(x, y)| {
            (x.r - y.r).abs() > 1e-4 || (x.g - y.g).abs() > 1e-4 || (x.b - y.b).abs() > 1e-4
        })
        .count()
}

/// Total red-minus-green across the frame — a probe tinting the scene red
/// raises this, and a change that merely brightens everything does not.
fn red_bias(pixels: &[Color]) -> f32 {
    pixels.iter().map(|c| c.r - c.g).sum()
}

/// **The acceptance.** A scene inside a `ReflectionProbe` whose interior ambient
/// is red renders red-tinted pixels; the identical scene without the probe does
/// not.
#[test]
fn reflection_probe_decal_scene_render() {
    let vp = viewport();

    // -- (1) the probe is collected, with the authored ambient ---------------
    let tree = load(RED_PROBE);
    let inputs = editor_3d_render_inputs(&vp, &tree);
    assert_eq!(
        inputs.env_effects.reflection_probes.len(),
        1,
        "the `.tscn` ReflectionProbe must reach the render inputs"
    );
    let probe = &inputs.env_effects.reflection_probes[0];
    assert_eq!(
        probe.ambient_mode,
        ReflectionProbeAmbientMode::ConstantColor,
        "`ambient_mode = 2` must survive the parse — it gates the probe's whole \
         contribution in `sample_probes_at`, and it was never extracted"
    );
    assert_eq!(
        (probe.ambient_color.r, probe.ambient_color.g),
        (1.0, 0.0),
        "the authored red ambient must survive the parse"
    );
    assert!(
        probe.interior,
        "`interior = true` must survive the parse, got {}",
        probe.interior
    );

    // -- (2) …and it reaches the shaded frame --------------------------------
    let with_probe = render_editor_3d_frame(&vp, &tree);
    // The same probe with its ambient off (see `RED_PROBE_AMBIENT_OFF`): the
    // reflection and the editor marker are common-mode, so what differs is the
    // ambient term alone.
    let without = render_editor_3d_frame(&vp, &load(RED_PROBE_AMBIENT_OFF));
    assert_eq!(
        with_probe.pixels.len(),
        without.pixels.len(),
        "both frames are the same size"
    );

    let changed = differing_pixels(&with_probe.pixels, &without.pixels);
    assert!(
        changed > 0,
        "a red-ambient probe containing the floor must change pixels; before this \
         change `sample_reflection_probes` had zero production callers, so the \
         probe was collected, transformed and handed to the renderer and still \
         lit nothing"
    );

    // Specifically *red*-tinted, not merely different. A probe that brightened
    // the frame uniformly would pass the count check above.
    //
    // ⚠️ pat-t42ca: Godot's ConstantColor probe REPLACES the environment
    // ambient inside its box (`reflection_process` REFLECTION_AMBIENT_COLOR,
    // then `ambient_light = ambient_accum.rgb`), it does not add to it. The
    // grey environment ambient becomes pure red, so every touched pixel keeps
    // or raises red and LOSES green and blue. (This used to assert g/b stay
    // put, which encoded the additive misconception.)
    let mut reddened = 0usize;
    for (with, base) in with_probe.pixels.iter().zip(without.pixels.iter()) {
        if (with.r - base.r).abs() <= 1e-4
            && (with.g - base.g).abs() <= 1e-4
            && (with.b - base.b).abs() <= 1e-4
        {
            continue; // untouched
        }
        assert!(
            with.r >= base.r - 1e-4,
            "a red ambient may not lower red: {base:?} -> {with:?}"
        );
        assert!(
            with.g <= base.g + 1e-4 && with.b <= base.b + 1e-4,
            "a red ambient REPLACING a grey one can only remove green and \
             blue: {base:?} -> {with:?}"
        );
        reddened += 1;
    }
    assert_eq!(
        reddened, changed,
        "every pixel the probe changed must be one it reddened"
    );

    // The frame-wide bias is the weaker, human-readable form of the same claim.
    let tinted = red_bias(&with_probe.pixels);
    let plain = red_bias(&without.pixels);
    assert!(
        tinted > plain,
        "the probe's red ambient must bias the frame toward red: {tinted} vs {plain}"
    );
}

/// `ambient_mode = 0` (Disabled) must actually disable the probe's AMBIENT — the
/// clause that makes the test above attributable to the ambient rather than to
/// the probe's mere presence.
///
/// ⚠️ The control is a default probe, not "no probe". Disabling the ambient
/// does not disable the probe: Godot still applies its captured REFLECTION
/// (`ReflectionProbe.xml`: `ambient_mode` only "sets how the probe's ambient
/// light is computed"), and the editor still draws the node's marker. Both are
/// identical between these two scenes, so a red `ambient_color` leaking past
/// `ambient_mode = 0` is the only thing that could differ.
#[test]
fn a_disabled_probe_contributes_nothing() {
    let vp = viewport();
    let disabled = render_editor_3d_frame(&vp, &load(DISABLED_PROBE));
    // pat-t42ca: the control is a Disabled probe with a BLACK ambient colour.
    // A default (Environment) probe is no longer a valid control: in Godot it
    // replaces the ambient with its blurred capture.
    let default_probe =
        render_editor_3d_frame(&vp, &load(&format!("{DEFAULT_PROBE}ambient_mode = 0\n")));

    // Guard: the floor must actually draw, or two empty frames are "identical".
    let empty = render_editor_3d_frame(&vp, &load(&DEFAULT_PROBE.replace(
        "mesh = \"primitive:PlaneMesh\"",
        "",
    )));
    assert!(
        differing_pixels(&default_probe.pixels, &empty.pixels) > 0,
        "fixture check: the floor must render, or this comparison is vacuous"
    );

    assert_eq!(
        differing_pixels(&disabled.pixels, &default_probe.pixels),
        0,
        "a probe with `ambient_mode = 0` must render identically to a \
         black-ambient disabled probe, even with a red `ambient_color` authored"
    );
}

/// A probe left at Godot's defaults (`ambient_mode = ENVIRONMENT`) REPLACES
/// the ambient with its blurred capture (pat-t42ca), so its frame differs from
/// the same probe with its ambient off — and its `ambient_color` is ignored:
/// Godot reads `ambient_color` only in ConstantColor mode.
///
/// ⚠️ This test used to assert the default probe changed NOTHING, encoding
/// the old additive `ambient_color * energy` (black) model; Godot measured on
/// platformer_3d says otherwise (shadowed floor 69 with probes disabled, 40
/// with the default mode).
#[test]
fn a_default_probe_replaces_the_ambient_with_its_capture() {
    let vp = viewport();
    let default_probe = load(DEFAULT_PROBE);
    let framed = render_editor_3d_frame(&vp, &default_probe);
    let ambient_off = render_editor_3d_frame(
        &vp,
        &load(&format!("{DEFAULT_PROBE}ambient_mode = 0\n")),
    );

    // Guard the fixture actually produced a probe, or this passes vacuously.
    let inputs = editor_3d_render_inputs(&vp, &default_probe);
    assert_eq!(
        inputs.env_effects.reflection_probes.len(),
        1,
        "the default-valued probe must be in the scene for this to mean anything"
    );
    assert_eq!(
        inputs.env_effects.reflection_probes[0].ambient_mode,
        ReflectionProbeAmbientMode::Environment,
        "Godot's default `ambient_mode` is ENVIRONMENT (ReflectionProbe.xml:23)"
    );

    assert!(
        differing_pixels(&framed.pixels, &ambient_off.pixels) > 0,
        "an Environment-mode probe must replace the ambient with its capture, \
         so the frame must differ from the same probe with ambient_mode = 0"
    );
    let red_colour = render_editor_3d_frame(
        &vp,
        &load(&format!("{DEFAULT_PROBE}ambient_color = Color(1, 0, 0, 1)\n")),
    );
    assert_eq!(
        differing_pixels(&framed.pixels, &red_colour.pixels),
        0,
        "Environment mode must ignore `ambient_color`; adding it is the old \
         additive model Godot never had"
    );
}

/// The decal clause: the four projected texture channels survive the `.tscn`,
/// and the decal changes the floor pixels under its box.
#[test]
fn decal_texture_channels_and_floor_pixels() {
    let vp = viewport();
    let tree = load(TEXTURED_DECAL);
    let inputs = editor_3d_render_inputs(&vp, &tree);

    assert_eq!(
        inputs.env_effects.decals.len(),
        1,
        "the `.tscn` Decal must reach the render inputs"
    );
    let decal = &inputs.env_effects.decals[0];
    assert_eq!(
        decal.texture_albedo.as_deref(),
        Some("res://splat_albedo.png"),
        "the albedo channel must survive; `Decal3D` has carried these path \
         fields all along but nothing read them off a node"
    );
    assert_eq!(
        decal.texture_normal.as_deref(),
        Some("res://splat_normal.png"),
        "the normal channel must survive"
    );
    assert_eq!(
        decal.texture_orm.as_deref(),
        Some("res://splat_orm.png"),
        "the ORM channel must survive"
    );
    assert_eq!(
        decal.texture_emission.as_deref(),
        Some("res://splat_emission.png"),
        "the emission channel must survive"
    );

    // A decal with no textures named leaves them `None` rather than `Some("")`.
    let bare = load(NO_PROBE);
    assert!(
        editor_3d_render_inputs(&vp, &bare)
            .env_effects
            .decals
            .is_empty(),
        "the control scene has no decal"
    );

    let with_decal = render_editor_3d_frame(&vp, &tree);
    let without = render_editor_3d_frame(&vp, &load(NO_PROBE));
    let changed = differing_pixels(&with_decal.pixels, &without.pixels);
    assert!(
        changed > 0,
        "the decal must change floor pixels under its box"
    );
    assert!(
        changed < with_decal.pixels.len(),
        "the decal must be bounded by its box, not repaint every pixel \
         ({changed} of {})",
        with_decal.pixels.len()
    );
}

/// The class-surface clause. `Decal` / `ReflectionProbe` / `VoxelGI` /
/// `LightmapGI` were registered with a name and a parent and nothing else, and
/// `LightmapProbe` was not registered at all.
#[test]
fn env_nodes_have_their_registered_property_surface() {
    // ⚠️ The ClassDB is not populated in unit tests unless a register entry
    // point runs first; without this every assertion below passes vacuously in
    // the wrong direction (empty list == "not registered").
    gdobject::class_db::register_3d_classes();
    use gdobject::class_db::{class_exists, get_property_list, property_get_revert};

    // Nothing may regress to a bare registration.
    for class in ["Decal", "ReflectionProbe", "VoxelGI", "LightmapGI"] {
        let own = get_property_list(class).len();
        let base = get_property_list("VisualInstance3D").len();
        assert!(
            own > base,
            "{class} must declare properties of its own, not just inherit \
             VisualInstance3D's ({own} vs {base})"
        );
    }

    // `LightmapProbe` was missing entirely — instantiating one from a `.tscn`
    // or the create dialog failed outright. It inherits Node3D, NOT
    // VisualInstance3D, and genuinely has no properties of its own.
    assert!(
        class_exists("LightmapProbe"),
        "LightmapProbe must be registered"
    );
    assert!(
        gdobject::class_db::inheritance_chain("LightmapProbe")
            .iter()
            .any(|c| c == "Node3D"),
        "LightmapProbe inherits Node3D"
    );

    // Spot-check the defaults that are easy to get wrong, against the doc XML.
    assert_eq!(
        property_get_revert("Decal", "upper_fade"),
        Some(gdvariant::Variant::Float(0.3)),
        "Decal.upper_fade defaults to 0.3, not 0.0 — a 0.0 default makes the \
         decal edge hard instead of feathered"
    );
    assert_eq!(
        property_get_revert("Decal", "cull_mask"),
        Some(gdvariant::Variant::Int(1_048_575)),
        "cull_mask is 0xFFFFF (20 visual layers), not all-ones"
    );
    assert_eq!(
        property_get_revert("ReflectionProbe", "ambient_mode"),
        Some(gdvariant::Variant::Int(1)),
        "ReflectionProbe.ambient_mode defaults to 1 (Environment); a 0 default \
         would make every authored probe contribute nothing"
    );
    assert_eq!(
        property_get_revert("LightmapGI", "use_denoiser"),
        Some(gdvariant::Variant::Bool(true)),
        "LightmapGI.use_denoiser defaults to true"
    );
    assert_eq!(
        property_get_revert("LightmapGI", "bias"),
        Some(gdvariant::Variant::Float(0.0005)),
        "LightmapGI.bias is 0.0005, an order of magnitude off the tempting 0.005"
    );

    // The four decal texture channels must be *declared*, or the inspector has
    // no slot to assign a texture into.
    for channel in [
        "texture_albedo",
        "texture_normal",
        "texture_orm",
        "texture_emission",
    ] {
        assert!(
            gdobject::class_db::property_can_revert("Decal", channel),
            "Decal must declare {channel}"
        );
    }
}

/// The determinism clause: hiding the probe drops it from the scenario, and the
/// frame returns exactly to the no-probe frame.
#[test]
fn hiding_the_probe_returns_the_original_frame() {
    let vp = viewport();
    let hidden = load(&RED_PROBE.replace(
        r#"[node name="Probe" type="ReflectionProbe" parent="."]"#,
        "[node name=\"Probe\" type=\"ReflectionProbe\" parent=\".\"]\nvisible = false",
    ));

    let inputs = editor_3d_render_inputs(&vp, &hidden);
    assert!(
        inputs.env_effects.reflection_probes.is_empty(),
        "a hidden VisualInstance3D leaves the scenario entirely"
    );

    let framed = render_editor_3d_frame(&vp, &hidden);
    let without = render_editor_3d_frame(&vp, &load(NO_PROBE));
    assert_eq!(
        differing_pixels(&framed.pixels, &without.pixels),
        0,
        "hiding the probe must return the frame to the no-probe baseline"
    );

    // …and rendering the same scene twice is deterministic, so the equalities
    // above are not an artifact of frame-to-frame noise.
    let again = render_editor_3d_frame(&vp, &load(NO_PROBE));
    assert_eq!(
        differing_pixels(&without.pixels, &again.pixels),
        0,
        "the software render must be deterministic"
    );
}
