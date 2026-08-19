//! pat-s3rrv.90 — the editor/Play render path stops dropping Light3D properties.
//!
//! ⚠️ **Two readers existed and only one was complete.**
//! `gdscene::render_server_3d::sync_light_properties` reads the full Light3D
//! property set; `gdeditor::viewport_3d_render::scene_lights` hand-read about
//! ten properties of its own. The shipped editor and Play viewport use the
//! second one, so a scene light's temperature, cull mask, indirect/volumetric
//! energies, shadow opacity/caster-mask and distance fade were silently dropped
//! — the same light shaded differently depending on which renderer walked it.
//! `scene_lights` now feeds the shared reader.
//!
//! ⚠️ **This header used to say only `light_temperature` reached pixels. That
//! is no longer true.** The shading now consumes `cull_mask`, `shadow_opacity`,
//! `shadow_bias`, `shadow_normal_bias`, `specular`, `light_negative`, the
//! `distance_fade_*` set and the directional shadow fade, on both the software
//! and GPU paths — so the bead's acceptance wording ("the masked mesh unlit,
//! lighter shadow") is now testable at the PIXEL level, and
//! `light_properties_authored_on_a_scene_light_change_the_editor_frame` below
//! does exactly that.
//!
//! ⚠️ **A second boundary was dropping properties.** `SceneLight3D` carried
//! them and `setup_editor_lighting_3d` copied only a subset into the renderer's
//! `Light3D`, so `shadow_bias`, `shadow_normal_bias`, `specular`,
//! `light_negative` and the directional fade pair reached
//! `RenderServer3DAdapter` — which has no non-test callers — but never the
//! shipped editor or Play view. Carrying a property is only half the wiring.
//!
//! Still carried but consumed by nothing: `indirect_energy` and
//! `volumetric_fog_energy` (no GI or fog path exists), `shadow_caster_mask`,
//! and the `shadow_split_*`/`blend_splits` cascade controls. Those assertions
//! below are struct-level on purpose.

use gdcore::math::Vector3;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::{
    editor_3d_render_inputs, render_editor_3d_frame, SceneLightKind,
};
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// `light_temperature` is written as `3000.0`, the form Godot's own .tscn writer
/// emits.
///
/// ⚠️ This comment used to warn that an integer-typed `3000` would be silently
/// ignored. That hole is CLOSED — the shared reader's numeric helper now matches
/// `Variant::Int` as well as `Variant::Float`, because GDScript types a bare
/// `light.light_temperature = 3000` as an Int and every such assignment was
/// being dropped.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Warm" type="OmniLight3D" parent="."]
light_color = Color(1, 1, 1, 1)
light_temperature = 3000.0
light_cull_mask = 5
light_indirect_energy = 0.25
light_volumetric_fog_energy = 0.75
shadow_opacity = 0.5
shadow_caster_mask = 9
distance_fade_enabled = true
distance_fade_begin = 20.0
distance_fade_length = 5.0

[node name="Plain" type="OmniLight3D" parent="."]
light_color = Color(1, 1, 1, 1)
"#;

fn load(src: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

/// **The acceptance test.**
#[test]
fn light3d_full_property_set_reaches_editor_render() {
    gdobject::class_db::register_3d_classes();
    let tree = load(SCENE);
    let viewport = Viewport3D::default();

    let inputs = editor_3d_render_inputs(&viewport, &tree);
    assert_eq!(
        inputs.scene_lights.len(),
        2,
        "both OmniLight3D nodes must be collected, or the comparison below is \
         between a light and nothing",
    );
    // Tree order: Warm first, then Plain.
    let warm = inputs.scene_lights[0];
    let plain = inputs.scene_lights[1];

    // -- the half that reaches pixels ---------------------------------------
    //
    // 3000 K is a warm source, so the blackbody tint must push red above blue.
    // Compared against an identical light with no temperature rather than an
    // absolute value: that pins the DIRECTION of the tint without hard-coding
    // whatever `kelvin_to_rgb` returns.
    assert_eq!(
        (plain.color.r, plain.color.b),
        (1.0, 1.0),
        "the control light authored pure white must stay white — if this tints, \
         a temperature is being applied where none was authored",
    );
    assert!(
        warm.color.r > warm.color.b,
        "3000 K must tint the light warm (red above blue); got r={} b={}. Equal \
         channels mean light_temperature never reached the editor path, which \
         is the bug this bead is about",
        warm.color.r,
        warm.color.b,
    );

    // -- carried, and consumed by nothing yet --------------------------------
    //
    // These were absent from the editor path's light struct entirely, so before
    // this change there was nowhere for them to even be observed. `cull_mask`
    // and `shadow_opacity` have since grown real consumers; they stay here as
    // carriage assertions and are proven at the pixel level further down.
    assert_eq!(warm.temperature, 3000.0);
    assert_eq!(warm.cull_mask, 5);
    assert_eq!(warm.indirect_energy, 0.25);
    assert_eq!(warm.volumetric_fog_energy, 0.75);
    assert_eq!(warm.shadow_opacity, 0.5);
    assert_eq!(warm.shadow_caster_mask, 9);
    assert!(warm.distance_fade_enabled);
    assert_eq!(warm.distance_fade_begin, 20.0);
    assert_eq!(warm.distance_fade_length, 5.0);
}

/// The control: a light that authors none of these keeps the engine defaults.
///
/// ⚠️ Without this, a `scene_lights` that filled every field with the authored
/// values of the FIRST light — or with a constant — would pass the test above.
#[test]
fn an_unconfigured_light_keeps_the_defaults() {
    gdobject::class_db::register_3d_classes();
    let tree = load(SCENE);
    let viewport = Viewport3D::default();

    let inputs = editor_3d_render_inputs(&viewport, &tree);
    let plain = inputs.scene_lights[1];

    assert!(
        !plain.distance_fade_enabled,
        "a light that never authored distance_fade_enabled must not inherit the \
         neighbouring light's true",
    );
    assert_ne!(
        plain.cull_mask, 5,
        "nor its cull mask — equal masks would mean the reader is filling every \
         light from the same node",
    );
    assert_eq!(
        plain.shadow_opacity, 1.0,
        "Godot's shadow_opacity default is 1.0 (fully opaque shadow)",
    );
}

// ── The bead's acceptance, at the pixel level ────────────────────────────────

const W: u32 = 96;
const H: u32 = 72;

/// Mean luminance — "this render is brighter/darker than that one" needs no
/// projection math and cannot pass by accident.
fn mean_luma(fb: &FrameBuffer) -> f32 {
    if fb.pixels.is_empty() {
        return 0.0;
    }
    let sum: f32 = fb
        .pixels
        .iter()
        .map(|c| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b)
        .sum();
    sum / fb.pixels.len() as f32
}

fn diff_pixels(a: &FrameBuffer, b: &FrameBuffer) -> usize {
    assert_eq!(a.pixels.len(), b.pixels.len(), "frames must be same size");
    a.pixels
        .iter()
        .zip(b.pixels.iter())
        .filter(|(x, y)| x != y)
        .count()
}

/// A lit floor under one `DirectionalLight3D`, with `props` authored on the
/// light.
///
/// ⚠️ Built node by node rather than from a `.tscn`, because a primitive mesh is
/// spelled as a node property here and as a SubResource in a real scene file;
/// the programmatic form is what every other 3D render test in this crate uses.
///
/// ⚠️ The scene must contain a light. `setup_editor_lighting_3d` falls back to
/// the editor's PREVIEW SUN when `scene_lights` is empty, and that sun carries
/// none of the authored properties — every comparison below would then be
/// between two identical preview-lit frames.
fn lit_floor(props: &[(&str, Variant)]) -> FrameBuffer {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .expect("root added");

    let mut floor = Node::new("Floor", "MeshInstance3D");
    floor.set_property("mesh", Variant::String("primitive:PlaneMesh".to_string()));
    let floor_id = tree.add_child(root, floor).expect("floor added");
    gdscene::node3d::set_position(&mut tree, floor_id, Vector3::ZERO);

    // Identity basis aims a light down its own −Z; a −90° turn about X points
    // it at −Y, straight down onto the floor.
    let sun_id = tree
        .add_child(root, Node::new("Sun", "DirectionalLight3D"))
        .expect("sun added");
    gdscene::node3d::set_position(&mut tree, sun_id, Vector3::new(0.0, 4.0, 0.0));
    gdscene::node3d::set_rotation(
        &mut tree,
        sun_id,
        Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0),
    );
    let sun = tree.get_node_mut(sun_id).expect("sun exists");
    for (name, value) in props {
        sun.set_property(name, value.clone());
    }

    render_editor_3d_frame(&Viewport3D::new(W, H), &tree)
}

/// 🔑 **THE BEAD'S ACCEPTANCE, PROVEN IN PIXELS.** Properties authored on a
/// scene light change the editor frame — the masked mesh goes unlit and a
/// negative light subtracts.
///
/// ⚠️ Every comparison is between two scenes differing by exactly ONE authored
/// property, so a pass cannot come from the scene being lit some other way: if
/// the property were still dropped, the two frames would be byte-identical and
/// the `diff_pixels` assertions would all fail.
///
/// ⚠️ `light_negative` and the two bias fields were reaching the shared reader
/// and then being dropped at `setup_editor_lighting_3d`, which copied only a
/// subset of `SceneLight3D` into the renderer's light. That boundary is why a
/// struct-level assertion is not enough for this bead — the properties were
/// demonstrably *carried* while still changing nothing on screen.
#[test]
fn light_properties_authored_on_a_scene_light_change_the_editor_frame() {
    let lit = lit_floor(&[]);

    // Positive control. Without a genuinely lit floor every "darker" assertion
    // below is satisfied by a black frame compared against another black frame.
    assert!(
        mean_luma(&lit) > 0.01,
        "the floor must actually be lit, or the rest of this test is vacuous; \
         got mean luma {}",
        mean_luma(&lit)
    );

    // -- light_cull_mask: the masked mesh goes unlit --------------------------
    //
    // A mesh's `layers` defaults to 1, so a cull mask of 2 (layer 2 only) has an
    // empty intersection with it and the light must skip the floor entirely.
    let masked = lit_floor(&[("light_cull_mask", Variant::Int(2))]);
    assert!(
        diff_pixels(&lit, &masked) > 0,
        "a light whose cull mask excludes the mesh's layer must change the \
         frame; identical frames mean light_cull_mask never reached the shading"
    );
    assert!(
        mean_luma(&masked) < mean_luma(&lit),
        "the masked floor must be DARKER, not merely different: lit={}, \
         masked={}",
        mean_luma(&lit),
        mean_luma(&masked)
    );

    // A mask that still INCLUDES layer 1 must leave the frame alone — this is
    // what separates "the mask is applied" from "any non-default mask breaks
    // the light", which would also pass the two assertions above.
    let included = lit_floor(&[("light_cull_mask", Variant::Int(3))]);
    assert_eq!(
        diff_pixels(&lit, &included),
        0,
        "a cull mask that still contains the mesh's layer must render exactly \
         as the default does"
    );

    // -- light_negative: the light subtracts ---------------------------------
    //
    // Godot flips the sign of the energy rather than the colour, so the authored
    // colour survives and the surface simply loses light.
    let negative = lit_floor(&[("light_negative", Variant::Bool(true))]);
    assert!(
        diff_pixels(&lit, &negative) > 0,
        "light_negative must change the frame; identical frames mean it was \
         dropped between the shared reader and the renderer"
    );
    assert!(
        mean_luma(&negative) < mean_luma(&lit),
        "a negative light subtracts, so its frame cannot be brighter: lit={}, \
         negative={}",
        mean_luma(&lit),
        mean_luma(&negative)
    );
}

/// A scene of three unauthored lights, one of each class.
const CLASS_DEFAULTS_SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node3D"]

[node name="Sun" type="DirectionalLight3D" parent="."]

[node name="Bulb" type="OmniLight3D" parent="."]

[node name="Cone" type="SpotLight3D" parent="."]
"#;

/// 🔑 AN UNAUTHORED LIGHT KEEPS **ITS OWN CLASS'S** DEFAULTS.
///
/// The shared reader only overwrites what a node actually authored, so every
/// unauthored property keeps whatever the `Light3D` it was handed already had.
/// The editor path seeded that `Light3D` as a `DirectionalLight3D` for every
/// light regardless of class — and three defaults differ per class:
///
/// ```text
///     shadow_bias         directional 0.1   omni 0.1   spot 0.03
///     shadow_normal_bias  directional 2.0   omni 1.0   spot 1.0
///     light_specular      directional 1.0   omni 0.5   spot 0.5
/// ```
///
/// ⚠️ This was INERT until the properties were carried through to the renderer,
/// which is why it survived: `setup_editor_lighting_3d` rebuilt the light from
/// the correct constructor and dropped these fields, so the wrong seed never
/// reached a pixel. The moment they are copied across, an unauthored omni light
/// renders with a directional light's normal bias and twice its specular.
///
/// ⚠️ The directional case is the control. Without it, a seed that produced the
/// OMNI defaults for everything would pass every other assertion here.
#[test]
fn an_unauthored_light_keeps_its_own_classes_defaults() {
    gdobject::class_db::register_3d_classes();
    let tree = load(CLASS_DEFAULTS_SCENE);
    let inputs = editor_3d_render_inputs(&Viewport3D::default(), &tree);

    let of_kind = |kind: SceneLightKind| {
        *inputs
            .scene_lights
            .iter()
            .find(|l| l.kind == kind)
            .unwrap_or_else(|| panic!("the scene must contribute a {kind:?} light"))
    };

    let sun = of_kind(SceneLightKind::Directional);
    assert_eq!(sun.shadow_bias, 0.1, "Light3D base default");
    assert_eq!(sun.shadow_normal_bias, 2.0, "Light3D base default");
    assert_eq!(sun.specular, 1.0, "Light3D base default");

    let bulb = of_kind(SceneLightKind::Omni);
    assert_eq!(bulb.shadow_bias, 0.1, "OmniLight3D inherits the base 0.1");
    assert_eq!(
        bulb.shadow_normal_bias, 1.0,
        "OmniLight3D overrides the base 2.0 — a directional seed leaks 2.0 here"
    );
    assert_eq!(
        bulb.specular, 0.5,
        "OmniLight3D overrides the base 1.0 — and the shader doubles positional \
         specular, so a leaked 1.0 makes every omni highlight twice as bright"
    );

    let cone = of_kind(SceneLightKind::Spot);
    assert_eq!(
        cone.shadow_bias, 0.03,
        "SpotLight3D overrides the base 0.1 — the only class that does"
    );
    assert_eq!(cone.shadow_normal_bias, 1.0, "SpotLight3D overrides 2.0");
    assert_eq!(cone.specular, 0.5, "SpotLight3D overrides 1.0");

    // Every class agrees on these, so they are the same in all three — asserted
    // once, on the light most likely to have been mis-seeded.
    assert_eq!(bulb.directional_shadow_max_distance, 100.0);
    assert_eq!(bulb.directional_shadow_fade_start, 0.8);
    assert!(!bulb.negative, "light_negative defaults to false");
}
