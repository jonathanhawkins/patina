//! pat-nzgdu.76 — a `.tscn`'s `GeometryInstance3D` properties survive the scene
//! sync and land on the render instance.
//!
//! The editor-viewport acceptance test
//! (`gdeditor/tests/geometryinstance_cast_shadow_and_visibility_range_consumed`)
//! proves the *pixels* change. This one pins the runtime sync layer
//! (`RenderServer3DAdapter`) that the running game uses, and pins the parsing
//! rules that a pixel test cannot distinguish: enum ordinals vs. bools, ints
//! standing in for floats, and the Godot defaults for an unset property.
//!
//! ```text
//! .tscn  ->  PackedScene  ->  SceneTree
//!   ->  RenderServer3DAdapter::render_frame (sync)  ->  Instance3D.geometry
//! ```

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdserver3d::instance::{Instance3D, ShadowCastingSetting, VisibilityRangeFadeMode};

const W: u32 = 32;
const H: u32 = 32;

fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// Renders one frame and returns the single mesh instance the sync produced.
fn synced_instance(tscn: &str) -> Instance3D {
    let tree = load(tscn);
    let mut adapter = RenderServer3DAdapter::new(W, H);
    let _ = adapter.render_frame(&tree);
    let instances = adapter.instances();
    assert_eq!(
        instances.len(),
        1,
        "expected exactly one synced mesh instance, got {}",
        instances.len()
    );
    instances[0].clone()
}

const HEADER: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 4)
current = true

[node name="Mesh" type="MeshInstance3D" parent="."]
"#;

/// A node that sets none of these properties must keep Godot's defaults —
/// otherwise introducing the settings block would silently change every
/// existing scene.
#[test]
fn unset_properties_keep_godot_defaults() {
    let inst = synced_instance(HEADER);
    let g = &inst.geometry;

    assert_eq!(g.cast_shadow, ShadowCastingSetting::On);
    assert_eq!(g.transparency, 0.0);
    assert_eq!(g.visibility_range_begin, 0.0);
    assert_eq!(g.visibility_range_end, 0.0);
    assert_eq!(
        g.visibility_range_fade_mode,
        VisibilityRangeFadeMode::Disabled
    );
    assert_eq!(g.lod_bias, 1.0);
    assert_eq!(g.extra_cull_margin, 0.0);
    assert!(g.material_overlay.is_none());
    assert!(inst.casts_shadow(), "the default must still cast");
    assert!(inst.draws_geometry());
}

/// A `.tscn` writes `cast_shadow` as Godot's enum ordinal. Each ordinal has to
/// map to the right mode — swapping `Off` and `ShadowsOnly` would produce the
/// exact inverse render.
#[test]
fn cast_shadow_ordinal_from_tscn_reaches_the_instance() {
    for (ordinal, expected) in [
        (0, ShadowCastingSetting::Off),
        (1, ShadowCastingSetting::On),
        (2, ShadowCastingSetting::DoubleSided),
        (3, ShadowCastingSetting::ShadowsOnly),
    ] {
        let inst = synced_instance(&format!("{HEADER}cast_shadow = {ordinal}\n"));
        assert_eq!(
            inst.geometry.cast_shadow, expected,
            "cast_shadow = {ordinal}"
        );
    }
}

/// ⚠️ Patina's own `node3d::set_cast_shadow` writes a **Bool**, not an ordinal.
/// A scene built through the scripting API must honour the setting too, so both
/// spellings have to be accepted.
#[test]
fn cast_shadow_bool_spelling_is_also_honoured() {
    let off = synced_instance(&format!("{HEADER}cast_shadow = false\n"));
    assert_eq!(off.geometry.cast_shadow, ShadowCastingSetting::Off);
    assert!(!off.casts_shadow());
    assert!(
        off.draws_geometry(),
        "cast_shadow = false must not hide the mesh"
    );

    let on = synced_instance(&format!("{HEADER}cast_shadow = true\n"));
    assert_eq!(on.geometry.cast_shadow, ShadowCastingSetting::On);
}

/// The visibility range and its margins reach the instance and drive the
/// distance test — the property block is useless if it only round-trips.
#[test]
fn visibility_range_reaches_the_instance_and_culls() {
    let tscn = format!(
        "{HEADER}visibility_range_begin = 2.0\nvisibility_range_end = 20.0\n\
         visibility_range_begin_margin = 1.0\nvisibility_range_end_margin = 3.0\n\
         visibility_range_fade_mode = 1\n"
    );
    let inst = synced_instance(&tscn);
    let g = &inst.geometry;

    assert_eq!(g.visibility_range_begin, 2.0);
    assert_eq!(g.visibility_range_end, 20.0);
    assert_eq!(g.visibility_range_begin_margin, 1.0);
    assert_eq!(g.visibility_range_end_margin, 3.0);
    assert_eq!(
        g.visibility_range_fade_mode,
        VisibilityRangeFadeMode::SelfFade
    );

    // The mesh sits at the origin, so the camera distance is the camera's own
    // distance from it.
    //
    // ⚠️ The margins MOVE the thresholds — they are not decoration. With fading
    // on, Godot's `_visibility_range_check` tests `dist < begin - begin_margin`
    // and `dist > end + end_margin`, so the live window here is
    // `(2 - 1) .. (20 + 3)` = `1 .. 23`, NOT `2 .. 20`. Picking test distances
    // off the nominal range lands on the wrong side of the real boundary.
    use gdcore::math::Vector3;
    assert!(inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 10.0)));
    assert!(
        !inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 24.0)),
        "24 units is past visibility_range_end + margin = 23"
    );
    assert!(
        inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 22.0)),
        "22 units is still inside the end margin (end + margin = 23)"
    );
    assert!(
        !inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 0.5)),
        "0.5 units is nearer than visibility_range_begin - margin = 1"
    );
    assert!(
        inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 1.5)),
        "1.5 units is inside the begin margin (begin - margin = 1)"
    );
}

/// With the margins at zero the window is exactly `begin .. end`, which is the
/// form the culling in the render loop actually runs in for a typical scene.
#[test]
fn visibility_range_without_margins_is_the_plain_window() {
    let inst = synced_instance(&format!(
        "{HEADER}visibility_range_begin = 2.0\nvisibility_range_end = 20.0\n"
    ));

    use gdcore::math::Vector3;
    assert!(inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 10.0)));
    assert!(
        !inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 21.0)),
        "21 units is past visibility_range_end = 20"
    );
    assert!(
        !inst.is_within_visibility_range(Vector3::new(0.0, 0.0, 1.0)),
        "1 unit is nearer than visibility_range_begin = 2"
    );
}

/// ⚠️ A `.tscn` writes a whole number without a decimal point as an **Int**
/// (`lod_bias = 2`), so a float-only read would drop exactly the values authors
/// are most likely to type.
#[test]
fn whole_number_properties_parse_as_ints() {
    let inst = synced_instance(&format!(
        "{HEADER}lod_bias = 2\nextra_cull_margin = 4\nvisibility_range_end = 30\n"
    ));
    assert_eq!(inst.geometry.lod_bias, 2.0);
    assert_eq!(inst.geometry.extra_cull_margin, 4.0);
    assert_eq!(inst.geometry.visibility_range_end, 30.0);
}

/// `transparency` is clamped to Godot's `0..1` range, and a fully transparent
/// instance reports zero opacity.
#[test]
fn transparency_is_clamped_and_drives_opacity() {
    let half = synced_instance(&format!("{HEADER}transparency = 0.25\n"));
    assert!((half.geometry.opacity_at_distance(0.0) - 0.75).abs() < 1e-6);

    let over = synced_instance(&format!("{HEADER}transparency = 5.0\n"));
    assert_eq!(over.geometry.transparency, 1.0, "must clamp to 1.0");
    assert_eq!(over.geometry.opacity_at_distance(0.0), 0.0);
}

/// `extra_cull_margin` grows the bounds culling reads; a negative value must be
/// ignored rather than shrinking them.
#[test]
fn extra_cull_margin_only_ever_grows_the_bounds() {
    let plain = synced_instance(HEADER);
    let grown = synced_instance(&format!("{HEADER}extra_cull_margin = 2.0\n"));
    let negative = synced_instance(&format!("{HEADER}extra_cull_margin = -2.0\n"));

    let base = plain.world_aabb().expect("the default cube has bounds");
    let big = grown.world_aabb().unwrap();
    assert!(
        big.size.x > base.size.x,
        "extra_cull_margin must widen the cull bounds: {} vs {}",
        big.size.x,
        base.size.x
    );
    assert_eq!(
        negative.world_aabb().unwrap().size.x,
        base.size.x,
        "a negative margin must be ignored, never shrink the bounds"
    );
}
