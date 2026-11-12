//! pat-s3rrv.71 — the H/V directional Control variants are in ClassDB.
//!
//! The bead names 23 classes; 17 were already registered. The six that were not
//! are all *directional* variants — `HFlowContainer`, `VFlowContainer`,
//! `HScrollBar`, `VScrollBar`, `HSeparator`, `VSeparator` — which is a coherent
//! gap rather than a scattering: the base classes had been done and the axis
//! specialisations skipped.
//!
//! ⚠️ Five of the six declare no members of their own, so registration is name +
//! parent. `VScrollBar` is the exception and it was found by reading
//! `doc/classes/*.xml` rather than reasoning from the names: it overrides the
//! two size flags, exactly as `VSlider` does. Registering all six uniformly
//! would have left a vertical scrollbar hugging the wrong axis — a wrong layout
//! that still renders.

use gdobject::class_db::{class_exists, get_class_info, register_2d_classes};

/// Every name the bead lists, so a future regression in ANY of them is caught
/// here and not only in the six that were missing.
const CONTROL_FAMILY: &[&str] = &[
    "AspectRatioContainer",
    "CenterContainer",
    "CheckButton",
    "FlowContainer",
    "HFlowContainer",
    "VFlowContainer",
    "LinkButton",
    "NinePatchRect",
    "TextureButton",
    "TextureProgressBar",
    "Slider",
    "HSlider",
    "VSlider",
    "ScrollBar",
    "HScrollBar",
    "VScrollBar",
    "SpinBox",
    "Tree",
    "PopupPanel",
    "HSeparator",
    "VSeparator",
];

/// ⚠️ `register_2d_classes()` ALONE, as the acceptance specifies. Calling
/// `register_all_classes()` here would pass even if these lived in some other
/// family's registrar, which is exactly the mistake this pins.
#[test]
fn the_control_family_registers_from_register_2d_classes_alone() {
    register_2d_classes();
    for name in CONTROL_FAMILY {
        assert!(
            class_exists(name),
            "{name} must be registered by register_2d_classes() alone"
        );
    }
}

/// Each inheritance chain must terminate at Object rather than dangling on an
/// unregistered base — a chain that stops early makes `is_class("Control")`
/// answer false for a class that plainly is one.
#[test]
fn every_chain_terminates_at_object() {
    register_2d_classes();
    for name in CONTROL_FAMILY {
        let mut cursor = (*name).to_string();
        let mut hops = 0;
        loop {
            assert!(
                hops < 16,
                "{name}'s inheritance chain did not terminate within 16 hops \
                 (cycle or runaway parent)"
            );
            let info = get_class_info(&cursor)
                .unwrap_or_else(|| panic!("{name}: ancestor `{cursor}` is not registered"));
            if info.parent_class.is_empty() || cursor == "Object" {
                break;
            }
            cursor = info.parent_class;
            hops += 1;
        }
        assert_eq!(
            cursor, "Object",
            "{name} must ultimately derive from Object"
        );
    }
}

/// The directional parents are the specialisation Godot declares.
#[test]
fn the_directional_variants_specialise_the_right_base() {
    register_2d_classes();
    for (name, parent) in [
        ("HFlowContainer", "FlowContainer"),
        ("VFlowContainer", "FlowContainer"),
        ("HScrollBar", "ScrollBar"),
        ("VScrollBar", "ScrollBar"),
        ("HSeparator", "Separator"),
        ("VSeparator", "Separator"),
    ] {
        let info = get_class_info(name).unwrap_or_else(|| panic!("{name} registered"));
        assert_eq!(info.parent_class, parent, "{name} specialises {parent}");
    }
}

/// ⚠️ The asymmetry, pinned. VScrollBar carries the two size-flag overrides its
/// doc XML declares; HScrollBar carries neither, because its own XML declares
/// none and it takes ScrollBar's defaults.
///
/// Asserting only VScrollBar would pass against a uniform registration that gave
/// the flags to BOTH, so the negative half is what makes this meaningful.
#[test]
fn vscrollbar_overrides_its_size_flags_and_hscrollbar_does_not() {
    register_2d_classes();
    let v = get_class_info("VScrollBar").expect("VScrollBar registered");
    let flag = |info: &gdobject::class_db::ClassInfo, key: &str| -> Option<i64> {
        info.properties
            .iter()
            .find(|p| p.name == key)
            .map(|p| match p.default_value {
                gdvariant::Variant::Int(i) => i,
                _ => panic!("{key} must be an int size-flag bitfield"),
            })
    };
    assert_eq!(
        flag(&v, "size_flags_horizontal"),
        Some(0),
        "a vertical scrollbar HUGS horizontally (doc XML default 0)"
    );
    assert_eq!(
        flag(&v, "size_flags_vertical"),
        Some(1),
        "and FILLS vertically (doc XML default 1)"
    );

    let h = get_class_info("HScrollBar").expect("HScrollBar registered");
    assert_eq!(
        flag(&h, "size_flags_horizontal"),
        None,
        "HScrollBar's doc XML declares no members, so it must NOT carry an \
         invented override — it inherits ScrollBar's defaults"
    );
}
