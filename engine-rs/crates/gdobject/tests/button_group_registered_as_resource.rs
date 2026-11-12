//! pat-s3rrv.161 — `ButtonGroup` resolves in ClassDB as a `Resource`.
//!
//! Godot ground truth: `upstream/godot/doc/classes/ButtonGroup.xml`
//! (`inherits="Resource"`, `allow_unpress` default `false` with getter
//! `is_allow_unpress`, and `resource_local_to_scene` overriding Resource's
//! default with `true`).
//!
//! ⚠️ **Registration and defaults only.** The radio behaviour itself lives in
//! `gdscene` and is proven by `button_group_enforces_radio_exclusivity`, which
//! drives real clicks. A green run here means a `.tscn` naming a `ButtonGroup`
//! sub-resource resolves and the inspector shows the right defaults.
//!
//! ⚠️ This file also asserts what is deliberately **absent**. `get_buttons()`,
//! `get_pressed_button()` and the `pressed(button)` signal are documented on
//! this class but need the ButtonGroup itself as the receiver, and a Patina
//! resource is a value with no object identity — `BaseButton.button_group`
//! reads as `Nil`, so no script can obtain the group to call or connect to it.
//! Declaring them would advertise a surface the engine cannot dispatch. If that
//! gap is ever closed, these three assertions are the ones to delete, and they
//! name the reason so nobody deletes them by accident.

use gdobject::class_db::{
    class_exists, class_has_method, get_property_list, get_signal_list, register_all_classes,
};
use gdobject::is_parent_class;
use gdvariant::Variant;

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

#[test]
fn button_group_is_registered_as_a_resource() {
    register_all_classes();

    assert!(
        class_exists("ButtonGroup"),
        "a .tscn writing `[sub_resource type=\"ButtonGroup\"]` must resolve"
    );
    assert!(
        is_parent_class("ButtonGroup", "Resource"),
        "ButtonGroup inherits Resource — not Control, and not Object directly"
    );
}

#[test]
fn button_group_carries_its_documented_defaults() {
    register_all_classes();

    assert_eq!(
        default_of("ButtonGroup", "allow_unpress"),
        Some(Variant::Bool(false)),
        "allow_unpress defaults to false: by default the active member of a \
         group cannot be clicked off, which is what makes it a radio group"
    );

    // ⚠️ Resource's own default is false. Getting this wrong is invisible until
    // two instances of one scene start sharing a single radio group.
    assert_eq!(
        default_of("ButtonGroup", "resource_local_to_scene"),
        Some(Variant::Bool(true)),
        "ButtonGroup OVERRIDES Resource's default with true"
    );
}

/// The accessors are named as Godot names them.
///
/// ⚠️ `is_allow_unpress`, not `get_allow_unpress` — a bool member whose getter
/// breaks the `get_` convention. Deriving the name instead of transcribing it
/// from the doc XML produces a plausible, wrong string.
#[test]
fn allow_unpress_uses_godots_accessor_names() {
    register_all_classes();

    let prop = get_property_list("ButtonGroup")
        .into_iter()
        .find(|p| p.name == "allow_unpress")
        .expect("allow_unpress must be registered");

    assert_eq!(prop.getter, "is_allow_unpress");
    assert_eq!(prop.setter, "set_allow_unpress");
}

/// 🔑 The absence is deliberate, not an oversight — see the module note.
#[test]
fn the_receiver_only_surface_is_not_advertised() {
    register_all_classes();

    for method in ["get_buttons", "get_pressed_button"] {
        assert!(
            !class_has_method("ButtonGroup", method),
            "`{method}` needs the ButtonGroup as a receiver, and a Patina \
             resource has no object identity for a script to reach. Declaring \
             it would advertise a call the engine cannot dispatch. If resources \
             gain identity, implement the dispatch and delete this assertion \
             together — never the assertion alone"
        );
    }

    assert!(
        !get_signal_list("ButtonGroup")
            .into_iter()
            .any(|s| s.name == "pressed"),
        "`pressed(button)` is emitted BY the group object, which does not exist \
         as an object here; `gdscene::control::button_group_pressed_button` \
         carries the same fact in a form callers can actually observe"
    );
}
