//! pat-s3rrv.165 — the FoldableContainer family resolves in ClassDB.
//!
//! Ground truth: `upstream/godot/doc/classes/FoldableContainer.xml` and
//! `FoldableGroup.xml`.
//!
//! ⚠️ Registration and defaults only. The folding behaviour is proven by
//! `foldable_container_group_exclusive_folding` in `gdscene`.
//!
//! ⚠️ This file also asserts what is deliberately **absent** on `FoldableGroup`:
//! `get_containers()`, `get_expanded_container()` and the `expanded(container)`
//! signal all need the group itself as a receiver, and a Patina resource is a
//! value with no object identity — `foldable_group` reads as `Nil`, so no script
//! can obtain the group to call or connect to it. Declaring them would advertise
//! a surface the engine cannot dispatch. Same situation as `ButtonGroup`
//! (pat-s3rrv.161). If resources ever gain identity, implement the dispatch and
//! delete those assertions together — never the assertions alone.

use gdobject::class_db::{
    class_exists, class_has_method, get_property_list, get_signal_list, register_all_classes,
};
use gdobject::is_parent_class;
use gdvariant::Variant;

fn prop(class_name: &str, property: &str) -> Option<gdobject::class_db::PropertyInfo> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
}

#[test]
fn the_foldable_family_is_registered_under_its_godot_parents() {
    register_all_classes();

    assert!(class_exists("FoldableContainer"));
    assert!(
        is_parent_class("FoldableContainer", "Container"),
        "FoldableContainer inherits Container"
    );
    assert!(class_exists("FoldableGroup"));
    assert!(
        is_parent_class("FoldableGroup", "Resource"),
        "FoldableGroup inherits Resource — it is a resource, not a node"
    );
}

/// The container's own members, with the defaults Godot documents.
#[test]
fn foldable_container_carries_its_documented_defaults() {
    register_all_classes();

    assert_eq!(
        prop("FoldableContainer", "folded").map(|p| p.default_value),
        Some(Variant::Bool(false)),
        "a container starts expanded"
    );
    // ⚠️ Both of these OVERRIDE Control's defaults, per
    // `foldable_container.cpp:599-600`. `mouse_filter = 0` is STOP: Container
    // sets PASS, and FoldableContainer sets it straight back, because the title
    // bar is a click target that has to win the hit test.
    assert_eq!(
        prop("FoldableContainer", "focus_mode").map(|p| p.default_value),
        Some(Variant::Int(2)),
        "FOCUS_ALL, overriding Control's FOCUS_NONE"
    );
    assert_eq!(
        prop("FoldableContainer", "mouse_filter").map(|p| p.default_value),
        Some(Variant::Int(0)),
        "MOUSE_FILTER_STOP — if this is PASS the title bar is unclickable"
    );

    let group = prop("FoldableContainer", "foldable_group")
        .expect("foldable_group must be registered — it is what makes folding exclusive");
    assert_eq!(group.getter, "get_foldable_group");
    assert_eq!(group.setter, "set_foldable_group");

    assert!(
        get_signal_list("FoldableContainer")
            .into_iter()
            .any(|s| s.name == "folding_changed"),
        "folding_changed is emitted by the container, which is a real object"
    );
}

/// ⚠️ `is_allow_folding_all`, not `get_allow_folding_all` — a bool member whose
/// getter breaks the `get_` convention. Deriving the name instead of
/// transcribing it from the doc XML produces a plausible, wrong string. This is
/// the third member in this subsystem with that shape (`is_allow_unpress`,
/// `is_showing_hidden_files`).
#[test]
fn allow_folding_all_uses_godots_accessor_names() {
    register_all_classes();

    let p = prop("FoldableGroup", "allow_folding_all").expect("allow_folding_all is registered");
    assert_eq!(
        p.default_value,
        Variant::Bool(false),
        "false by default: a group always keeps one container open"
    );
    assert_eq!(p.getter, "is_allow_folding_all");
    assert_eq!(p.setter, "set_allow_folding_all");

    // ⚠️ Resource's own default is false. Getting this wrong is invisible until
    // two instances of one scene start sharing a single fold group.
    assert_eq!(
        prop("FoldableGroup", "resource_local_to_scene").map(|p| p.default_value),
        Some(Variant::Bool(true)),
        "FoldableGroup OVERRIDES Resource's default with true"
    );
}

/// 🔑 The absence is deliberate, not an oversight — see the module note.
#[test]
fn the_receiver_only_surface_is_not_advertised() {
    register_all_classes();

    for method in ["get_containers", "get_expanded_container"] {
        assert!(
            !class_has_method("FoldableGroup", method),
            "`{method}` needs the FoldableGroup as a receiver, and a Patina \
             resource has no object identity for a script to reach. Declaring \
             it would advertise a call the engine cannot dispatch"
        );
    }

    assert!(
        !get_signal_list("FoldableGroup")
            .into_iter()
            .any(|s| s.name == "expanded"),
        "`expanded(container)` is emitted BY the group object, which does not \
         exist as an object here; `control::foldable_group_expanded` carries the \
         same fact in a form callers can observe"
    );
}
