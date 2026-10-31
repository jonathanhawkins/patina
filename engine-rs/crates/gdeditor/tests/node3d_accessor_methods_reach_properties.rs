
//! pat-mgixk.31: `set_X`/`get_X`/`is_X` calls on 3D nodes reach their
//! properties, including the INDEXED `Light3D.set_param`/`get_param`.
//!
//! ⚠️ The bead's premise is partly stale and it is worth saying where, because
//! the fix is in a different place than the bead points at. The generic
//! accessor bridge **already exists**: `scripting.rs:5244` calls
//! `core_node_methods::call_accessor_backed_method`, which reverse-looks-up a
//! method against ClassDB's **registered** `PropertyInfo::getter`/`setter`
//! strings. The bead (and its verifier note) searched for
//! `strip_prefix("set_")` / `get_property_setter` and concluded there was no
//! fallback — but a prefix-stripping bridge is exactly what that code
//! deliberately is not, because the `get_`/`is_`/`has_` convention holds for
//! only 51.7% of Godot's documented members.
//!
//! Two things were genuinely missing, and both are measurable:
//!
//! 1. **Nothing to match on.** `PropertyInfo::new` leaves `getter`/`setter`
//!    EMPTY and `with_accessors` must be called explicitly, so a property
//!    registered without it is invisible to the reverse lookup. 478 of the 997
//!    registered 3D properties — 48% — had no accessor names, which is why
//!    `set_X` on a 3D node answered "unknown method" while the identical call
//!    on a Control worked.
//!
//! 2. **The indexed accessors.** Godot's light parameters are ONE `set_param`
//!    keyed by a `Light3D.Param` constant. `Light3D.PARAM_ENERGY` did not
//!    resolve at all (an `UndefinedVariable`), so no script could even name the
//!    parameter.
//!
//! 🔑 And the indexed case was not merely absent — it was WRONG, which is the
//! assertion this file cares most about. `light_energy` registers `set_param`
//! as its setter, so the generic reverse lookup matched `set_param` and the
//! `Accessor::Set` arm wrote `args[0]`. For an indexed call `args[0]` is the
//! INDEX, so `set_param(PARAM_ATTENUATION, 8.0)` wrote `light_energy = 6`:
//! correct for exactly one index, and a silent corruption of brightness for
//! every other one. `energy_after_attenuation` below is the guard for that, and
//! note the constant it uses — `PARAM_RANGE` would have been useless here,
//! because its index is 4 and the bead's authored energy is 4.0, so the bug
//! would have produced the right number by coincidence.

use gdcore::math3d::{Basis, Transform3D};
use gdeditor::viewport_3d_render::{
    game_3d_render_inputs, viewport_for_scene_camera, SceneLight3D, SceneLightKind,
};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::{GDScriptNodeInstance, MainLoop};
use gdvariant::Variant;

const W: u32 = 64;
const H: u32 = 48;

/// Progress the script drives the follower to, and the value the Rust-side
/// reference follower is given directly.
const PROGRESS: f64 = 2.0;

/// A scene-loaded OmniLight3D (the bead's wording), a Camera3D so the Play
/// frame has something to render from, and a Path3D carrying three
/// PathFollow3D children: one driven by the script, one driven from Rust as the
/// reference, and one left at the default so the comparison cannot be vacuous.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Bulb" type="OmniLight3D" parent="."]

[node name="Cam" type="Camera3D" parent="."]

[node name="Fx" type="GPUParticles3D" parent="."]

[node name="Pivot" type="Node3D" parent="."]

[node name="Path" type="Path3D" parent="."]

[node name="Follow" type="PathFollow3D" parent="Path"]

[node name="Ref" type="PathFollow3D" parent="Path"]

[node name="Zero" type="PathFollow3D" parent="Path"]
"#;

/// Each statement in `_ready` bumps `stage`, so an abort names the call that
/// raised instead of leaving every later assertion looking independently
/// broken. An undispatched method raises and aborts the whole handler.
const STAGES: [&str; 12] = [
    "$Bulb.set_param(Light3D.PARAM_ENERGY, 4.0)",
    "$Bulb.get_param(Light3D.PARAM_ENERGY)",
    "$Bulb.set_param(Light3D.PARAM_ATTENUATION, 8.0)",
    "$Bulb.get_param(Light3D.PARAM_ATTENUATION)",
    "$Bulb.get_param(Light3D.PARAM_ENERGY) (re-read)",
    "$Path/Follow.set_progress(2.0)",
    "$Pivot.set_as_top_level(true)",
    "$Pivot.is_set_as_top_level()",
    "$Fx.set_emitting(false)",
    "$Fx.set_amount(32)",
    "$Fx.is_emitting()",
    "$Fx.get_amount()",
];

/// ⚠️ `set_as_top_level` / `is_set_as_top_level` are in here deliberately: the
/// property is `top_level`, so NO prefix rule produces either name. They pass
/// only against the registered strings, which is the `is_X` half of the bead.
const SCRIPT: &str = "\
extends Node3D
var stage = 0
var energy_read = 0.0
var attenuation_read = 0.0
var energy_after_attenuation = 0.0
var top_level_read = false
var emitting_read = true
var amount_read = 0
var ran = false

func _ready():
    $Bulb.set_param(Light3D.PARAM_ENERGY, 4.0)
    stage = 1
    energy_read = $Bulb.get_param(Light3D.PARAM_ENERGY)
    stage = 2
    $Bulb.set_param(Light3D.PARAM_ATTENUATION, 8.0)
    stage = 3
    attenuation_read = $Bulb.get_param(Light3D.PARAM_ATTENUATION)
    stage = 4
    energy_after_attenuation = $Bulb.get_param(Light3D.PARAM_ENERGY)
    stage = 5
    $Path/Follow.set_progress(2.0)
    stage = 6
    $Pivot.set_as_top_level(true)
    stage = 7
    top_level_read = $Pivot.is_set_as_top_level()
    stage = 8
    $Fx.set_emitting(false)
    stage = 9
    $Fx.set_amount(32)
    stage = 10
    emitting_read = $Fx.is_emitting()
    stage = 11
    amount_read = $Fx.get_amount()
    stage = 12
    ran = true
";

fn script_var(tree: &SceneTree, node: NodeId, name: &str) -> Variant {
    use gdscript_interop::bindings::ScriptInstance;
    tree.get_script(node)
        .expect("script attached")
        .list_properties()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("script has no var `{name}`"))
        .default_value
}

fn as_f64(v: Variant, what: &str) -> f64 {
    match v {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{what} should be a number, got {other:?}"),
    }
}

fn find(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("the scene must contain a node named {name}"))
}

fn only_omni(lights: &[SceneLight3D]) -> SceneLight3D {
    *lights
        .iter()
        .find(|l| l.kind == SceneLightKind::Omni)
        .expect("the scene must contribute its OmniLight3D")
}

#[test]
fn node3d_accessor_methods_reach_properties() {
    // ⚠️ Needed HERE, not merely somewhere in the file: nextest runs each
    // `#[test]` in its own process, so a sibling's registration never carries
    // over. The accessor lookup, `is_class` light collection and the
    // `Light3D.PARAM_*` constants all read this registry, and it starts EMPTY —
    // without this the script raises on the first call and every assertion
    // below fails for the wrong reason.
    gdobject::class_db::register_all_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let camera = find(&tree, "Cam");
    tree.get_node_mut(camera).unwrap().set_property(
        "transform",
        Variant::Transform3D(Transform3D {
            basis: Basis::IDENTITY,
            origin: gdcore::math::Vector3::new(0.0, 0.0, 6.0),
        }),
    );

    // A curve that BENDS, so a follower's position is a distinctive point
    // rather than something a stuck-at-zero node could coincidentally match.
    let path = find(&tree, "Path");
    tree.get_node_mut(path).unwrap().set_property(
        "curve_points",
        Variant::PackedVector3Array(vec![
            gdcore::math::Vector3::new(0.0, 0.0, 0.0),
            gdcore::math::Vector3::new(1.0, 0.0, 0.0),
            gdcore::math::Vector3::new(2.0, 1.0, 0.0),
            gdcore::math::Vector3::new(3.0, 3.0, 0.0),
        ]),
    );

    // The reference follower, driven the way the engine's own tests drive it.
    // The script's follower must land in the SAME place; comparing against this
    // rather than against a hand-derived coordinate means the assertion stays
    // true if the curve sampler is ever corrected, and fails the moment the
    // script path and the direct path disagree.
    let reference = find(&tree, "Ref");
    tree.get_node_mut(reference)
        .unwrap()
        .set_property("progress", Variant::Float(PROGRESS));

    let follow = find(&tree, "Follow");
    let zero = find(&tree, "Zero");
    let bulb = find(&tree, "Bulb");
    let fx = find(&tree, "Fx");

    let script = GDScriptNodeInstance::from_source(SCRIPT, scene_root).expect("script parses");
    tree.attach_script(scene_root, Box::new(script));

    let mut ml = MainLoop::new(tree);

    // ⚠️ Baseline BEFORE `_ready`, so "the Play frame shows energy 4" cannot be
    // satisfied by a default that happens to be 4. Light3D's registered default
    // is 1.0, and this pins that.
    let viewport = viewport_for_scene_camera(ml.tree(), camera, W, H);
    let before = only_omni(&game_3d_render_inputs(ml.tree(), camera, &viewport).scene_lights);
    assert!(
        (before.energy - 4.0).abs() > 1e-6,
        "the light must NOT already render at energy 4 before the script runs, \
         or the assertion below proves nothing. Got {}",
        before.energy
    );

    ml.tree_mut().process_script_ready(scene_root);

    // -- 0. did `_ready` finish? An undispatched method raises and aborts it,
    //       which otherwise looks identical to "the var was never set" -------
    let stage = match script_var(ml.tree(), scene_root, "stage") {
        Variant::Int(i) => i as usize,
        other => panic!("`stage` should be an Int, got {other:?}"),
    };
    assert_eq!(
        script_var(ml.tree(), scene_root, "ran"),
        Variant::Bool(true),
        "`_ready` aborted after stage {stage} ({}), so the call that raised is \
         `{}` — that accessor is not dispatched",
        STAGES
            .get(stage.saturating_sub(1))
            .copied()
            .unwrap_or("(none yet)"),
        STAGES.get(stage).copied().unwrap_or("(unknown)"),
    );

    // -- 1. the bead's prove-it: the Play frame's SceneLight3D shows energy 4 --
    //
    // This is the whole point of routing through `set_node_property` rather
    // than writing a key directly: `render_server_3d` reads `light_energy` via
    // `node3d::get_light_energy`, so a write that landed anywhere else would
    // leave this at the default and look like the setter was never wired.
    let after = only_omni(&game_3d_render_inputs(ml.tree(), camera, &viewport).scene_lights);
    assert!(
        (after.energy - 4.0).abs() < 1e-6,
        "set_param(PARAM_ENERGY, 4.0) must reach the property the renderer \
         reads: the Play frame's SceneLight3D should carry energy 4, got {}",
        after.energy
    );

    // -- 2. `get_param` returns the stored value rather than Nil -------------
    assert!(
        (as_f64(
            script_var(ml.tree(), scene_root, "energy_read"),
            "energy_read"
        ) - 4.0)
            .abs()
            < 1e-6,
        "get_param(PARAM_ENERGY) must answer the stored 4.0, not Nil — a Nil \
         reads as `no such property` to the interpreter, so `get_param(p) + 1` \
         would die mid-frame",
    );

    // -- 3. a SECOND, non-zero index reaches its OWN property ---------------
    assert!(
        (as_f64(
            script_var(ml.tree(), scene_root, "attenuation_read"),
            "attenuation_read"
        ) - 8.0)
            .abs()
            < 1e-6,
        "get_param(PARAM_ATTENUATION) must answer 8.0, proving the index is \
         decoded rather than every param aliasing light_energy",
    );
    assert!(
        (as_f64(
            ml.tree()
                .get_node(bulb)
                .expect("the Bulb node")
                .get_property("omni_attenuation"),
            "omni_attenuation"
        ) - 8.0)
            .abs()
            < 1e-6,
        "PARAM_ATTENUATION must land on `omni_attenuation` — the property the \
         engine reads. Asserting only on get_param would pass even if both \
         directions shared one private slot nothing else looks at",
    );

    // -- 4. 🔑 the index must not leak into the VALUE ------------------------
    assert!(
        (as_f64(
            script_var(ml.tree(), scene_root, "energy_after_attenuation"),
            "energy_after_attenuation"
        ) - 4.0)
            .abs()
            < 1e-6,
        "setting PARAM_ATTENUATION must leave PARAM_ENERGY at 4.0. The generic \
         reverse lookup matches `set_param` to light_energy and writes args[0], \
         which for an indexed call is the INDEX — so this reads 6.0 when the \
         indexed case is not handled ahead of it",
    );

    // -- 5. a plain registered accessor whose name no rule could derive ------
    assert_eq!(
        script_var(ml.tree(), scene_root, "top_level_read"),
        Variant::Bool(true),
        "is_set_as_top_level() must report the top_level written by \
         set_as_top_level(true); neither name is derivable from `top_level`",
    );

    // -- 6. set_progress moves the follower in the next tick -----------------
    ml.step(1.0 / 60.0);
    let moved = gdscene::node3d::get_position(ml.tree(), follow);
    let expected = gdscene::node3d::get_position(ml.tree(), reference);
    let unmoved = gdscene::node3d::get_position(ml.tree(), zero);
    assert!(
        (moved - unmoved).length() > 1e-3,
        "the script-driven follower must have MOVED off the default-progress \
         follower's position; both at {unmoved:?} means set_progress never \
         reached the engine",
    );
    assert!(
        (moved - expected).length() < 1e-4,
        "$Path/Follow.set_progress(2.0) must put the follower where progress \
         2.0 put the Rust-driven reference: {moved:?} vs {expected:?}",
    );

    // -- 7. the second registration batch, on a node the simulation reads -----
    //
    // `emitting` is the other shape of non-derivable name: the getter is
    // `is_emitting`, not `get_emitting`. And these two are not inert state —
    // `main_loop.rs:936-938` reads `emitting` and `amount` off the node when it
    // builds the frame's `ParticleEmitter`, so a write that missed the property
    // would leave the simulation on its defaults.
    assert_eq!(
        script_var(ml.tree(), scene_root, "emitting_read"),
        Variant::Bool(false),
        "is_emitting() must report the `emitting` written by set_emitting(false) \
         — Godot's particle nodes default it to TRUE, so this cannot pass by \
         reading an unset property",
    );
    assert_eq!(
        ml.tree()
            .get_node(fx)
            .expect("the Fx node")
            .get_property("emitting"),
        Variant::Bool(false),
        "set_emitting(false) must land on the `emitting` property itself — that \
         is the key main_loop reads, and asserting only through is_emitting() \
         would pass even if both sides shared a slot nothing else looks at",
    );
    assert_eq!(
        script_var(ml.tree(), scene_root, "amount_read"),
        Variant::Int(32),
        "get_amount() must read back the 32 written by set_amount(32)",
    );
}
