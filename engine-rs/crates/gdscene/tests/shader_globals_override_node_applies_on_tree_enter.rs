//! pat-s3rrv.209 — a `ShaderGlobalsOverride` node in the scene tree actually
//! overrides project global shader parameters, and reverts them when it leaves.
//!
//! Godot ground truth: `upstream/godot/scene/main/shader_globals_override.cpp`.
//! On `NOTIFICATION_ENTER_TREE` the node pushes every set `params/<name>`
//! property through `RenderingServer.global_shader_parameter_set_override`; on
//! `NOTIFICATION_EXIT_TREE` it clears them; only one active override wins and a
//! second one reports a configuration warning.
//!
//! # What was broken
//!
//! The class was registered as a bare `Node` with no behaviour anywhere —
//! `grep -rn ShaderGlobalsOverride --include=*.rs crates/` found only the
//! registration, a class-exists assertion, an inspector row and doc comments.
//! The store it is supposed to drive was complete and already modelled the
//! override slot separately from the base value, but its only caller was the
//! script-facing `RenderingServer` method in the interpreter. So placing the
//! node in a `.tscn` overrode nothing at all.
//!
//! # Why asserting the store is asserting what a shader reads
//!
//! `gdserver3d::shader::uniform_value` resolves a uniform by looking in the
//! material's own parameter map first and then falling through to
//! `gdvariant::shader_globals::get` — that fallback is precisely what makes
//! `global uniform vec4 tint;` read a project value. So the store IS the value
//! a shader sees. `shader_globals_override_value_reaches_a_shader_uniform_read`
//! closes that last link explicitly by evaluating a uniform expression.
//!
//! ⚠️ Every test uses its **own** global parameter name. The store is process
//! -wide, so tests sharing a name would race each other.
//!
//! ⚠️ Test names are chosen so the acceptance sorts FIRST. Test order is
//! alphabetical and a runner stopping at the first failure would otherwise
//! report a control's result while leaving the acceptance unrun.

use gdcore::math::Color;
use gdscene::node::Node;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::{shader_globals, Variant};

const RED: (f32, f32, f32) = (1.0, 0.0, 0.0);
const GREEN: (f32, f32, f32) = (0.0, 1.0, 0.0);

fn color(c: (f32, f32, f32)) -> Variant {
    Variant::Color(Color::new(c.0, c.1, c.2, 1.0))
}

/// A tree holding one `ShaderGlobalsOverride` that sets `params/<global>`.
///
/// ⚠️ The property is set on the node **before** it is added. `add_child`
/// enters the tree immediately, and the apply runs on `ENTER_TREE` — so a node
/// populated after insertion would apply an empty parameter set. This matches
/// what the `.tscn` loader does (`add_packed_scene_to_tree` assigns every
/// property to the detached node and only then calls `add_child`), which is why
/// the acceptance below can go through a real scene.
fn tree_with_override(global: &str, value: Variant) -> (SceneTree, gdscene::node::NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Override", "ShaderGlobalsOverride");
    node.set_property(&format!("params/{global}"), value);
    let id = tree.add_child(root, node).expect("add override node");
    (tree, id)
}

/// 🔑 **THE ACCEPTANCE.** A `.tscn` whose `ShaderGlobalsOverride` sets
/// `params/<name>` changes the value a shader reads for that global, as soon as
/// the scene enters the tree.
#[test]
fn shader_globals_override_node_applies_on_tree_enter() {
    let global = "tint_enter";

    // Stands in for `project.godot`'s `[shader_globals] tint = {...}`, which
    // `patina-runner`'s `project_boot` installs with exactly this call.
    shader_globals::add(global, "color", color(RED));
    assert_eq!(
        shader_globals::get(global),
        color(RED),
        "the project's declared value, before any override"
    );

    let src = format!(
        r#"[gd_scene format=3]

[node name="Root" type="Node3D"]

[node name="Override" type="ShaderGlobalsOverride" parent="."]
params/{global} = Color(0, 1, 0, 1)
"#
    );
    let packed = PackedScene::from_tscn(&src).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &packed).expect("instance scene");

    assert_eq!(
        shader_globals::get(global),
        color(GREEN),
        "entering the tree must push `params/{global}` into the global store. \
         Before this change the node had no behaviour whatsoever, so the value \
         stayed at the project's red and the node was pure decoration"
    );
}

/// The other half of the contract: leaving the tree restores the project value.
#[test]
fn shader_globals_override_reverts_on_exit_tree() {
    let global = "tint_exit";
    shader_globals::add(global, "color", color(RED));

    let (mut tree, id) = tree_with_override(global, color(GREEN));
    assert_eq!(
        shader_globals::get(global),
        color(GREEN),
        "applied on enter"
    );

    tree.remove_node(id).expect("remove override node");
    assert_eq!(
        shader_globals::get(global),
        color(RED),
        "removing the node must CLEAR the override, not leave the last value \
         stuck. The store models the override slot separately from the base \
         value precisely so the project value survives underneath"
    );
}

/// ⚠️ Only one override may win. Godot's node warns when a second is added
/// rather than letting the two fight.
///
/// The warning has to come from a **native** branch: a `ShaderGlobalsOverride`
/// placed from the editor has no script, and `node_configuration_warnings`
/// returned early for script-less nodes, so a warning hung off the script path
/// could never have fired.
#[test]
fn shader_globals_override_second_node_reports_a_configuration_warning() {
    let global = "tint_second";
    shader_globals::add(global, "color", color(RED));

    let (mut tree, first) = tree_with_override(global, color(GREEN));
    let root = tree.root_id();
    let mut second_node = Node::new("Second", "ShaderGlobalsOverride");
    second_node.set_property(&format!("params/{global}"), color((0.0, 0.0, 1.0)));
    let second = tree
        .add_child(root, second_node)
        .expect("add second override");

    assert!(
        tree.node_configuration_warnings(first).is_empty(),
        "the FIRST override is the active one and must not warn"
    );
    let warnings = tree.node_configuration_warnings(second);
    assert!(
        !warnings.is_empty(),
        "a second ShaderGlobalsOverride must report a configuration warning"
    );
    assert!(
        warnings[0].contains("ShaderGlobalsOverride"),
        "the warning should name the class it is about, got {warnings:?}"
    );

    // 🔑 And the warning is not cosmetic — the second node must actually be
    // inert. Without this, a build that warns and *also* applies would pass the
    // assertions above while silently letting the last node added win.
    assert_eq!(
        shader_globals::get(global),
        color(GREEN),
        "the first override's value must still be the live one"
    );
}

/// A global the override never names is left alone.
///
/// ⚠️ The control against a build that clears or rewrites the whole store on
/// enter/exit instead of only the parameters the node actually sets.
#[test]
fn shader_globals_override_untouched_globals_keep_their_value() {
    let owned = "tint_owned";
    let bystander = "tint_bystander";
    shader_globals::add(owned, "color", color(RED));
    shader_globals::add(bystander, "color", color((0.0, 0.0, 1.0)));

    let (mut tree, id) = tree_with_override(owned, color(GREEN));
    assert_eq!(
        shader_globals::get(bystander),
        color((0.0, 0.0, 1.0)),
        "a global the node does not name must be untouched while it is active"
    );

    tree.remove_node(id).expect("remove override node");
    assert_eq!(
        shader_globals::get(bystander),
        color((0.0, 0.0, 1.0)),
        "…and still untouched after it leaves"
    );
}

/// 🔑 Closes the last link: the overridden value is what a shader's uniform
/// lookup actually returns.
///
/// `uniform_value` checks the material's own parameters first and falls through
/// to the global store, which is how `global uniform` resolves at all. A float
/// global is used so the value is unambiguous through `eval_float_expr_3d`.
#[test]
fn shader_globals_override_value_reaches_a_shader_uniform_read() {
    use gdserver3d::shader::{eval_float_expr_3d, FloatExpr3D, FragmentContext3D};
    use std::collections::HashMap;

    let global = "wind_strength";
    shader_globals::add(global, "float", Variant::Float(1.0));

    let ctx = FragmentContext3D::default();
    let no_material_params: HashMap<String, Variant> = HashMap::new();
    let read = |ctx: &FragmentContext3D| {
        eval_float_expr_3d(
            &FloatExpr3D::Uniform(global.to_string()),
            ctx,
            &no_material_params,
        )
    };

    assert!(
        (read(&ctx) - 1.0).abs() < 1e-6,
        "the project value is what a shader reads before any override"
    );

    let (mut tree, id) = tree_with_override(global, Variant::Float(7.0));
    assert!(
        (read(&ctx) - 7.0).abs() < 1e-6,
        "with the node in the tree the shader must read the OVERRIDE, got {}",
        read(&ctx)
    );

    tree.remove_node(id).expect("remove override node");
    assert!(
        (read(&ctx) - 1.0).abs() < 1e-6,
        "and the project value again once the node leaves, got {}",
        read(&ctx)
    );
}
