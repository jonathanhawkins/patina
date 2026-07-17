//! pat-nzgdu.226 — the spatial scene-effect VisualShader nodes.
//!
//! `LinearSceneDepth`, `DistanceFade` and `ProximityFade` read shader built-ins
//! (`DEPTH_TEXTURE`, `VERTEX`, `INV_PROJECTION_MATRIX`) and emit a fixed snippet.
//!
//! ⚠️ **All three bind NOTHING** — zero methods, zero properties, zero enums.
//! The bead's "min/max distance ports" and "distance port" are graph **ports**, a
//! VisualShader concept; they are not ClassDB properties. Reading them as
//! properties would invent members Godot does not have. (This is the same shape
//! as pat-nzgdu.221's four empty placeholders and .213's `RenderDataRD`.)
//!
//! ⚠️ *"ClassDB has no shader classes"* is stale — **53 `VisualShader*` classes
//! were already registered** before this bead (49 before pat-nzgdu.225 added
//! four). So is the "PREREQUISITE (blocking): VisualShader/VisualShaderNode base
//! registration": both have been registered for some time.
//!
//! **`generate_code()` does not exist anywhere in the workspace** — there is no
//! VisualShader codegen at all, for *any* node — so the acceptance's "emits the
//! depth-linearization and fade expressions matching a Godot reference fixture"
//! has nothing to call. Same blocker as pat-nzgdu.225. See the bead notes.
//!
//! Schemas diffed against Godot 4.6.1.stable's `extension_api.json`;
//! constructibility against the `GDREGISTER_*` macros.

use gdobject::class_db::{can_instantiate, is_abstract};
use gdobject::GodotObject;
use gdobject::{
    class_exists, get_method_list, get_property_list, instantiate, is_parent_class,
    register_3d_classes,
};

/// The three the bead names.
const SCENE_EFFECT_NODES: &[&str] = &[
    "VisualShaderNodeLinearSceneDepth",
    "VisualShaderNodeDistanceFade",
    "VisualShaderNodeProximityFade",
];

fn own_methods(class: &str, base: &str) -> Vec<String> {
    let base_methods: Vec<String> = get_method_list(base).into_iter().map(|m| m.name).collect();
    get_method_list(class)
        .into_iter()
        .map(|m| m.name)
        .filter(|m| !base_methods.contains(m))
        .collect()
}

/// The acceptance: each registered, parented to `VisualShaderNode`, instantiable.
#[test]
fn visual_shader_scene_effect_nodes_test() {
    register_3d_classes();

    for node in SCENE_EFFECT_NODES {
        assert!(class_exists(node), "{node} must be registered");

        // "get_parent_class returns 'VisualShaderNode' for each"
        assert_eq!(
            gdobject::get_class_info(node)
                .expect("registered")
                .parent_class,
            "VisualShaderNode",
            "{node}'s immediate parent"
        );
        // An unregistered intermediate severs the chain silently (pat-nzgdu.98).
        assert!(is_parent_class(node, "Resource"), "{node} reaches Resource");
        assert!(is_parent_class(node, "Object"));

        // "and each is instantiable"
        assert!(!is_abstract(node), "{node} is GDREGISTER_CLASS in Godot");
        assert!(can_instantiate(node));
        let obj = instantiate(node).unwrap_or_else(|| panic!("{node} instantiates"));
        assert_eq!(obj.get_class(), *node);
    }
}

/// ⚠️ All three bind NOTHING — their inputs are graph PORTS, not properties.
///
/// `DistanceFade`'s min/max and `ProximityFade`'s distance are VisualShader graph
/// ports. Registering them as ClassDB properties would invent members Godot does
/// not have — and the bead's prose ("min/max distance ports") is exactly the kind
/// of phrasing that invites it.
#[test]
fn they_bind_nothing_because_their_inputs_are_ports_not_properties() {
    register_3d_classes();

    for node in SCENE_EFFECT_NODES {
        assert!(
            own_methods(node, "VisualShaderNode").is_empty(),
            "{node} binds no methods of its own in Godot"
        );

        let base_props: Vec<String> = get_property_list("VisualShaderNode")
            .into_iter()
            .map(|p| p.name)
            .collect();
        let own: Vec<String> = get_property_list(node)
            .into_iter()
            .map(|p| p.name)
            .filter(|p| !base_props.contains(p))
            .collect();
        assert!(
            own.is_empty(),
            "{node} binds no properties of its own; found {own:?} — ports are not \
             ClassDB properties"
        );
    }

    // Not vacuous: a sibling node in the same family DOES bind members, so an
    // empty own-set is a real fact about these three rather than about the
    // helper. (pat-nzgdu.225 registered this one.)
    assert!(
        !own_methods("VisualShaderNodeCubemap", "VisualShaderNode").is_empty(),
        "VisualShaderNodeCubemap binds six methods — the comparison is meaningful"
    );
}

/// They are plain `VisualShaderNode`s — not resizable, not parameters.
///
/// ⚠️ Worth pinning: the VisualShader family has several bases that a
/// scene-effect node could plausibly be filed under, and picking the wrong one
/// is silent. `VisualShaderNodeResizableBase` (pat-nzgdu.225) is for nodes that
/// draw a preview; these draw nothing.
#[test]
fn they_derive_visual_shader_node_directly() {
    register_3d_classes();

    for node in SCENE_EFFECT_NODES {
        assert!(
            !is_parent_class(node, "VisualShaderNodeResizableBase"),
            "{node} is not resizable — it has no preview to size"
        );
        assert!(
            !is_parent_class(node, "VisualShaderNodeParameter"),
            "{node} declares no uniform"
        );
    }

    // Not vacuous: both alternative bases exist and really do derive
    // VisualShaderNode, so the negatives above discriminate.
    assert!(class_exists("VisualShaderNodeResizableBase"));
    assert!(is_parent_class(
        "VisualShaderNodeResizableBase",
        "VisualShaderNode"
    ));
    assert!(class_exists("VisualShaderNodeParameter"));
    assert!(is_parent_class(
        "VisualShaderNodeParameter",
        "VisualShaderNode"
    ));
}

#[test]
fn registration_is_idempotent() {
    // `class_count()` and property lists cannot catch a duplicate registration
    // (`classes` is name-keyed, so re-registering overwrites). The ClassId is
    // what actually changes.
    let ids = || -> Vec<_> {
        SCENE_EFFECT_NODES
            .iter()
            .map(|c| gdobject::get_class_info(c).expect("registered").class_id)
            .collect()
    };

    register_3d_classes();
    let first = ids();
    register_3d_classes();
    assert_eq!(
        first,
        ids(),
        "re-registering must not mint new ClassIds — a guard is missing if it does"
    );
}
