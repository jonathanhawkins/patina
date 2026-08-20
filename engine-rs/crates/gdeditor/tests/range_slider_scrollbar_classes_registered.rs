//! pat-s3rrv.169 acceptance: the Range subtree resolves in ClassDB, the editor
//! can offer the concrete leaves, and a `.tscn` naming them loads.
//!
//! Ground truth: `upstream/godot/doc/classes/{Slider,HSlider,VSlider,ScrollBar,
//! HScrollBar,VScrollBar,SpinBox}.xml`. Godot's subtree is
//! `Range -> Slider -> {HSlider, VSlider}`, `Range -> ScrollBar -> {HScrollBar,
//! VScrollBar}` and `Range -> SpinBox`, with `Slider` and `ScrollBar` abstract.
//!
//! # Why this test lives in `gdeditor`
//!
//! The acceptance spans three layers — ClassDB registration, the editor's
//! Create-Node dialog, and loading a `.tscn` — and `gdeditor` is the only crate
//! that depends on all of them. Splitting it across crates would let the
//! registration pass while the thing it exists to enable stays broken, which is
//! precisely the gap this bead was filed for: the behaviour and theming already
//! worked, and only the ClassDB rung underneath them was missing.

use gdobject::class_db;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A scene naming both concrete leaves the acceptance calls out.
const SCENE: &str = r#"[gd_scene format=3]

[node name="Root" type="Control"]

[node name="Vol" type="VSlider" parent="."]

[node name="Bar" type="VScrollBar" parent="."]
"#;

fn default_of(class_name: &str, property: &str) -> Option<Variant> {
    class_db::get_property_list(class_name)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

/// Clause 1: the two bases are abstract and hang off `Range`.
///
/// ⚠️ Abstractness is not decoration here — the Create-Node dialog filters on
/// `is_abstract`, so a `Slider` marked concrete would be offered to users as a
/// placeable node that Godot does not allow in a scene.
#[test]
fn slider_and_scrollbar_are_abstract_bases_under_range() {
    class_db::register_all_classes();

    for base in ["Slider", "ScrollBar"] {
        assert!(class_db::class_exists(base), "{base} must be registered");
        assert!(
            class_db::is_abstract(base),
            "{base} is abstract in Godot — only its H/V leaves are placeable"
        );
        assert!(
            gdobject::is_parent_class(base, "Range"),
            "{base} inherits Range"
        );
    }
}

/// Clause 2: every leaf hangs off the right base.
///
/// ⚠️ `SpinBox` hangs off `Range` DIRECTLY, not off `Slider` — it is a spinner,
/// not a slider, and grouping it with its neighbours here is the easy mistake.
#[test]
fn the_concrete_leaves_hang_off_the_right_bases() {
    class_db::register_all_classes();

    for (leaf, base) in [
        ("HSlider", "Slider"),
        ("VSlider", "Slider"),
        ("HScrollBar", "ScrollBar"),
        ("VScrollBar", "ScrollBar"),
        ("SpinBox", "Range"),
    ] {
        assert!(class_db::class_exists(leaf), "{leaf} must be registered");
        assert!(
            !class_db::is_abstract(leaf),
            "{leaf} is concrete — a scene can contain one"
        );
        assert!(
            gdobject::is_parent_class(leaf, base),
            "{leaf} must inherit {base}"
        );
    }
}

/// 🔑 Clause 3, the acceptance's literal chain.
#[test]
fn range_slider_scrollbar_classes_registered() {
    class_db::register_all_classes();

    assert_eq!(
        class_db::inheritance_chain("VSlider"),
        vec![
            "VSlider",
            "Slider",
            "Range",
            "Control",
            "CanvasItem",
            "Node",
            "Object"
        ],
        "the full chain has to resolve: a break anywhere in it makes \
         `is_class(\"Control\")` false and drops the node out of every generic \
         Control path"
    );
    assert_eq!(
        class_db::inheritance_chain("VScrollBar"),
        vec![
            "VScrollBar",
            "ScrollBar",
            "Range",
            "Control",
            "CanvasItem",
            "Node",
            "Object"
        ]
    );
}

/// Clause 4: the doc-XML defaults each base carries.
///
/// ⚠️ These are the ones that are easy to get backwards. `ScrollBar.custom_step`
/// is **-1.0**, not 0 — negative means "derive from the page size", so a 0 would
/// silently make every scroll notch move nothing. `ScrollBar.step` is **0.0**
/// while `Slider.step` is **1.0**; they are siblings with opposite defaults.
#[test]
fn the_bases_carry_their_documented_defaults() {
    class_db::register_all_classes();

    assert_eq!(default_of("Slider", "editable"), Some(Variant::Bool(true)));
    assert_eq!(
        default_of("Slider", "scrollable"),
        Some(Variant::Bool(true))
    );
    assert_eq!(default_of("Slider", "tick_count"), Some(Variant::Int(0)));
    assert_eq!(
        default_of("Slider", "ticks_on_borders"),
        Some(Variant::Bool(false))
    );
    assert_eq!(
        default_of("Slider", "step"),
        Some(Variant::Float(1.0)),
        "a slider steps by 1 by default"
    );
    // FOCUS_ALL: a slider is keyboard-reachable, unlike most Controls.
    assert_eq!(default_of("Slider", "focus_mode"), Some(Variant::Int(2)));

    assert_eq!(
        default_of("ScrollBar", "custom_step"),
        Some(Variant::Float(-1.0)),
        "negative means 'derive from the page size'; 0.0 would move nothing"
    );
    assert_eq!(
        default_of("ScrollBar", "step"),
        Some(Variant::Float(0.0)),
        "a scrollbar's step is 0, the OPPOSITE of its Slider sibling's 1.0"
    );

    assert_eq!(default_of("SpinBox", "editable"), Some(Variant::Bool(true)));
    assert_eq!(
        default_of("SpinBox", "update_on_text_changed"),
        Some(Variant::Bool(false))
    );
    assert_eq!(
        default_of("SpinBox", "select_all_on_focus"),
        Some(Variant::Bool(false))
    );
    assert_eq!(
        default_of("SpinBox", "prefix"),
        Some(Variant::String(String::new()))
    );
    assert_eq!(
        default_of("SpinBox", "suffix"),
        Some(Variant::String(String::new()))
    );
}

/// 🔑 Clause 5: the editor can actually offer the leaves — the user-visible half.
///
/// This is what the bead was filed for. The behaviour and theming already
/// worked; without the ClassDB rung the Create-Node dialog, which enumerates
/// ClassDB, could not list a VSlider at all.
#[test]
fn the_create_dialog_offers_the_leaves_but_not_the_abstract_bases() {
    class_db::register_all_classes();

    let dialog = gdeditor::create_dialog::CreateNodeDialog::with_base_class("Control");
    let listed: Vec<String> = dialog
        .filtered_classes()
        .into_iter()
        .map(|e| e.class_name)
        .collect();

    for leaf in ["VSlider", "HSlider", "VScrollBar", "HScrollBar", "SpinBox"] {
        assert!(
            listed.contains(&leaf.to_string()),
            "the create dialog cannot offer {leaf}, so a user cannot add one"
        );
    }
    // ⚠️ The negative half. Godot's `_should_hide_type` keeps abstract classes
    // out of the list; offering `Slider` would let a user place a node the
    // engine cannot instantiate.
    for base in ["Slider", "ScrollBar"] {
        assert!(
            !listed.contains(&base.to_string()),
            "{base} is abstract and must NOT be offered as a placeable node"
        );
    }
}

/// Clause 6: a `.tscn` naming the leaves loads with both nodes typed.
#[test]
fn a_tscn_using_the_leaves_loads_with_both_nodes_typed() {
    class_db::register_all_classes();

    let scene = PackedScene::from_tscn(SCENE).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    for (name, class) in [("Vol", "VSlider"), ("Bar", "VScrollBar")] {
        let id = tree
            .all_nodes_in_tree_order()
            .into_iter()
            .find(|id| {
                tree.get_node(*id)
                    .map(|n| n.name() == name)
                    .unwrap_or(false)
            })
            .unwrap_or_else(|| panic!("no node named {name}"));
        let node = tree.get_node(id).expect("node exists");
        assert_eq!(node.class_name(), class);
        // ⚠️ Through ClassDB, not by name: this is the assertion that was false
        // before the subtree was registered, and it is what every generic
        // Control path — hit-testing, layout, theming — keys on.
        assert!(
            node.is_class("Control"),
            "{class} must resolve to Control through ClassDB"
        );
        assert!(
            node.is_class("Range"),
            "{class} must resolve to Range — the value/min/max surface lives there"
        );
    }
}
