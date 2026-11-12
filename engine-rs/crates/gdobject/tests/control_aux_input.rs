//! pat-0q2kx.175 — `Control`'s aux input surface in ClassDB.
//!
//! `Control` was registered with four anchor properties and `accept_event()` and
//! nothing else, so `tooltip_text` and `mouse_default_cursor_shape` did not exist
//! as far as the engine was concerned: a `.tscn` carrying either silently dropped
//! it, the inspector had nothing to show, and there was no value for a hover
//! timer or a cursor resolver to read.
//!
//! ⚠️ **Schema only.** Registering these does not make a tooltip appear or a
//! cursor change — the hover timer, the tooltip draw layer, the
//! `FrameOutput` cursor field and the drag flow are all still unimplemented.
//! These tests pin the schema so those stages have something to read, and
//! deliberately assert nothing about behaviour.

use gdobject::class_db::{class_has_enum, class_has_property, get_class_info, register_2d_classes};
use gdobject::{class_exists, is_parent_class};
use gdvariant::Variant;

fn default_of(class: &str, name: &str) -> Variant {
    get_class_info(class)
        .unwrap_or_else(|| panic!("{class} is registered"))
        .properties
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{class} has no property `{name}`"))
        .default_value
        .clone()
}

/// The two properties, with Godot's documented defaults.
#[test]
fn control_declares_tooltip_text_and_cursor_shape() {
    register_2d_classes();
    assert!(class_exists("Control"));

    // `default=""` in doc/classes/Control.xml.
    assert_eq!(
        default_of("Control", "tooltip_text"),
        Variant::String(String::new()),
        "tooltip_text defaults to the empty string"
    );

    // `default="0"` = CURSOR_ARROW. Enum-zero genuinely IS the default for this
    // one, which is worth pinning precisely because it usually is not.
    assert_eq!(
        default_of("Control", "mouse_default_cursor_shape"),
        Variant::Int(0),
        "mouse_default_cursor_shape defaults to CURSOR_ARROW (0)"
    );
}

/// `Control.CursorShape` must be a real enum with all 17 of Godot's shapes in
/// declaration order — a partial list would let a `.tscn` round-trip a shape the
/// engine cannot name.
#[test]
fn cursor_shape_enum_has_all_seventeen_shapes() {
    register_2d_classes();

    assert!(
        class_has_enum("Control", "CursorShape"),
        "the cursor shapes must be a real enum, not bare ints"
    );

    let constants = gdobject::class_db::get_enum_constants("Control", "CursorShape");
    assert_eq!(
        constants.len(),
        17,
        "Godot declares 17 CURSOR_* shapes; got {constants:?}"
    );
    for expected in [
        "CURSOR_ARROW",
        "CURSOR_IBEAM",
        "CURSOR_POINTING_HAND",
        "CURSOR_CROSS",
        "CURSOR_WAIT",
        "CURSOR_BUSY",
        "CURSOR_DRAG",
        "CURSOR_CAN_DROP",
        "CURSOR_FORBIDDEN",
        "CURSOR_VSIZE",
        "CURSOR_HSIZE",
        "CURSOR_BDIAGSIZE",
        "CURSOR_FDIAGSIZE",
        "CURSOR_MOVE",
        "CURSOR_VSPLIT",
        "CURSOR_HSPLIT",
        "CURSOR_HELP",
    ] {
        assert!(
            constants.iter().any(|c| c == expected),
            "missing {expected}; got {constants:?}"
        );
    }
}

/// Subclasses must **inherit** both rather than needing their own copies.
///
/// This is the clause that matters for the acceptance test: it hovers a
/// **Button**, so `tooltip_text` has to resolve through the chain. Registering it
/// on `Control` alone is only useful if the lookup walks parents.
#[test]
fn control_subclasses_inherit_the_aux_input_surface() {
    register_2d_classes();

    for class in ["Button", "Label", "LineEdit"] {
        assert!(is_parent_class(class, "Control"), "{class} derives Control");
        assert!(
            class_has_property(class, "tooltip_text"),
            "{class} must inherit tooltip_text — the acceptance test hovers a Button"
        );
        assert!(
            class_has_property(class, "mouse_default_cursor_shape"),
            "{class} must inherit mouse_default_cursor_shape"
        );
        // Inherited, not redeclared: a subclass with its own copy would shadow
        // the base and drift from it.
        let own = get_class_info(class)
            .unwrap_or_else(|| panic!("{class} registered"))
            .properties;
        assert!(
            !own.iter().any(|p| p.name == "tooltip_text"),
            "{class} must not redeclare tooltip_text, got {:?}",
            own.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
    }
}
