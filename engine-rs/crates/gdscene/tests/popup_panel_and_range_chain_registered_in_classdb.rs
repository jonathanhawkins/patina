//! pat-s3rrv.168 acceptance: PopupPanel and the Range chain are real ClassDB
//! entries, and `CONTROL_HIT_CLASSES` is redundant because of it.
//!
//! Most of this was already registered — I surveyed the whole file rather than
//! a window, the lesson from pat-s3rrv.162. What the acceptance adds is
//! *proof*: that the chains resolve, that the abstract/instantiable split is
//! right, and that two overriding defaults are the values Godot documents
//! rather than the ones inherited from Range.

use gdobject::class_db;
use gdvariant::Variant;

/// Clause 1: PopupPanel's chain reaches Window, not just Popup.
#[test]
fn popup_panel_resolves_through_popup_to_window() {
    class_db::register_all_classes();

    assert!(class_db::class_exists("PopupPanel"));
    assert!(class_db::is_parent_class("PopupPanel", "Popup"));
    // Window is two links up. A chain walk that stopped at the immediate
    // parent would pass the Popup check and fail this one.
    assert!(class_db::is_parent_class("PopupPanel", "Window"));
}

/// Clause 2: every Range control resolves to both Range and Control.
#[test]
fn the_range_chain_resolves_to_range_and_control() {
    class_db::register_all_classes();

    for name in [
        "ScrollBar",
        "HScrollBar",
        "VScrollBar",
        "Slider",
        "HSlider",
        "VSlider",
        "SpinBox",
    ] {
        assert!(
            class_db::is_parent_class(name, "Range"),
            "{name} must descend from Range"
        );
        // Control is further up, through Range. Both are asserted because a
        // registration parented straight to Control would satisfy the second
        // and break the first.
        assert!(
            class_db::is_parent_class(name, "Control"),
            "{name} must descend from Control"
        );
    }
}

/// Clause 3: the bases are abstract, the leaves are not.
#[test]
fn scrollbar_and_slider_are_abstract_but_their_leaves_are_not() {
    class_db::register_all_classes();

    for base in ["ScrollBar", "Slider"] {
        assert!(
            class_db::is_abstract(base),
            "{base} is a base class and must not be instantiable"
        );
    }
    // ⚠️ The leaves must NOT inherit abstractness. Marking the base without
    // checking the leaves is how a whole family becomes uncreatable — the
    // editor's create dialog filters on exactly this flag.
    for leaf in ["HScrollBar", "VScrollBar", "HSlider", "VSlider", "SpinBox"] {
        assert!(
            !class_db::is_abstract(leaf),
            "{leaf} is a concrete control and must be instantiable"
        );
    }
}

/// Clause 4: two defaults that OVERRIDE what Range hands down.
#[test]
fn overriding_defaults_match_the_documented_values() {
    class_db::register_all_classes();

    // ScrollBar.custom_step = -1.0 (doc/classes/ScrollBar.xml). Not a Range
    // member at all, so a missing declaration reads as Nil rather than a wrong
    // number.
    assert_eq!(
        class_db::property_get_revert("HScrollBar", "custom_step"),
        Some(Variant::Float(-1.0)),
        "HScrollBar must report custom_step = -1.0"
    );

    // ⚠️ SpinBox.step OVERRIDES Range: doc/classes/SpinBox.xml says 1.0 where
    // Range.xml says 0.01. Inheriting the base value silently gives a spin box
    // that steps by a hundredth — plausible-looking and wrong.
    assert_eq!(
        class_db::property_get_revert("SpinBox", "step"),
        Some(Variant::Float(1.0)),
        "SpinBox must override Range's step with 1.0"
    );
}

/// Clause 5: the hard-coded hit-test list is now redundant.
#[test]
fn every_control_hit_class_resolves_through_classdb() {
    class_db::register_all_classes();

    let mut unresolved = Vec::new();
    for name in gdscene::control::CONTROL_HIT_CLASSES {
        let resolves = *name == "Control" || class_db::is_parent_class(name, "Control");
        if !resolves {
            unresolved.push(*name);
        }
    }

    assert!(
        unresolved.is_empty(),
        "these names only hit-test because of the hard-coded fallback — ClassDB \
         does not know them as Control descendants: {unresolved:?}"
    );
}
