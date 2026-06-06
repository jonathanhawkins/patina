//! pat-p21kp: Property inspector with typed editors for all Variant types.
//!
//! Integration tests covering:
//! 1. InspectorPanel — inspect, list, get/set properties, callbacks
//! 2. PropertyCategory — categorization for all known property names
//! 3. PropertyEditor — for_variant_type mapping for all VariantType variants
//! 4. EditorHint — range, enum, multiline, file, resource refinements
//! 5. Validation and coercion — validate_variant, coerce_variant
//! 6. SectionedInspector — category grouping, collapse/expand, visible count
//! 7. ResourceSubEditor — inline resource editing, undo, subresources
//! 8. set_property_validated — type-checked editing with coercion fallback

use std::sync::Arc;

use gdeditor::inspector::{PropertyCategory, PropertyEntry};
use gdeditor::{
    coerce_variant, validate_variant, EditorHint, InspectorPanel, InspectorSection, PropertyEditor,
    ResourceSubEditor, SectionedInspector,
};
use gdresource::Resource;
use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;
use gdvariant::{Variant, VariantType};

// ===========================================================================
// Helpers
// ===========================================================================

fn make_tree_with_props() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Player", "CharacterBody2D");
    node.set_property("position", Variant::Int(0));
    node.set_property("velocity", Variant::Float(1.5));
    node.set_property("visible", Variant::Bool(true));
    node.set_property("texture", Variant::String("res://icon.png".into()));
    node.set_property("mass", Variant::Float(10.0));
    node.set_property("scale", Variant::Int(1));
    node.set_property("custom_data", Variant::Nil);
    let id = tree.add_child(root, node).unwrap();
    (tree, id)
}

fn make_resource(class: &str, props: Vec<(&str, Variant)>) -> Arc<Resource> {
    let mut res = Resource::new(class);
    for (k, v) in props {
        res.set_property(k, v);
    }
    Arc::new(res)
}

// ===========================================================================
// 1. InspectorPanel — basics
// ===========================================================================

#[test]
fn inspector_inspect_and_clear() {
    let (tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();

    assert!(panel.inspected_node().is_none());
    panel.inspect(node_id);
    assert_eq!(panel.inspected_node(), Some(node_id));

    let props = panel.list_properties(&tree);
    assert!(!props.is_empty());

    panel.clear();
    assert!(panel.inspected_node().is_none());
    assert!(panel.list_properties(&tree).is_empty());
}

#[test]
fn inspector_list_properties_sorted() {
    let (tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let props = panel.list_properties(&tree);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "properties should be alphabetically sorted");
}

#[test]
fn inspector_get_set_property() {
    let (mut tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    // Get existing
    assert_eq!(panel.get_property(&tree, "position"), Variant::Int(0));

    // Set and verify old value returned
    let old = panel.set_property(&mut tree, "position", Variant::Int(42));
    assert_eq!(old, Variant::Int(0));
    assert_eq!(panel.get_property(&tree, "position"), Variant::Int(42));
}

#[test]
fn inspector_get_property_no_node_returns_nil() {
    let tree = SceneTree::new();
    let panel = InspectorPanel::new();
    assert_eq!(panel.get_property(&tree, "anything"), Variant::Nil);
}

#[test]
fn inspector_set_property_no_node_returns_nil() {
    let mut tree = SceneTree::new();
    let panel = InspectorPanel::new();
    let old = panel.set_property(&mut tree, "anything", Variant::Int(1));
    assert_eq!(old, Variant::Nil);
}

#[test]
fn inspector_property_changed_callback() {
    let (mut tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let fired_clone = fired.clone();
    panel.on_property_changed(move |name, _old, _new| {
        if name == "velocity" {
            fired_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });

    panel.set_property(&mut tree, "velocity", Variant::Float(9.0));
    assert!(fired.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn inspector_specific_property_callback_only_fires_for_match() {
    let (mut tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count_clone = count.clone();
    panel.on_specific_property_changed("mass", move |_name, _old, _new| {
        count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });

    // Changing a different property should NOT fire
    panel.set_property(&mut tree, "velocity", Variant::Float(2.0));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);

    // Changing the matched property should fire
    panel.set_property(&mut tree, "mass", Variant::Float(20.0));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}

// ===========================================================================
// 2. PropertyCategory — categorization
// ===========================================================================

#[test]
fn category_transform_properties() {
    for name in &[
        "position",
        "rotation",
        "scale",
        "transform",
        "global_position",
        "skew",
    ] {
        assert_eq!(
            PropertyCategory::categorize(name),
            PropertyCategory::Transform,
            "{name} should be Transform"
        );
    }
}

#[test]
fn category_rendering_properties() {
    for name in &[
        "visible", "modulate", "texture", "color", "z_index", "material",
    ] {
        assert_eq!(
            PropertyCategory::categorize(name),
            PropertyCategory::Rendering,
            "{name} should be Rendering"
        );
    }
}

#[test]
fn category_physics_properties() {
    for name in &["velocity", "mass", "gravity_scale", "friction", "bounce"] {
        assert_eq!(
            PropertyCategory::categorize(name),
            PropertyCategory::Physics,
            "{name} should be Physics"
        );
    }
}

#[test]
fn category_script_properties() {
    assert_eq!(
        PropertyCategory::categorize("script_var"),
        PropertyCategory::Script
    );
    assert_eq!(
        PropertyCategory::categorize("metadata/foo"),
        PropertyCategory::Script
    );
}

#[test]
fn category_misc_fallback() {
    assert_eq!(
        PropertyCategory::categorize("some_custom_thing"),
        PropertyCategory::Misc
    );
    assert_eq!(PropertyCategory::categorize(""), PropertyCategory::Misc);
}

#[test]
fn list_properties_by_category() {
    let (tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let grouped = panel.list_properties_by_category(&tree);
    // We have position/scale (Transform), visible/texture (Rendering),
    // velocity/mass (Physics), custom_data (Misc)
    assert!(grouped.contains_key(&PropertyCategory::Transform));
    assert!(grouped.contains_key(&PropertyCategory::Rendering));
    assert!(grouped.contains_key(&PropertyCategory::Physics));
}

// ===========================================================================
// 3. PropertyEditor — for_variant_type covers all VariantType variants
// ===========================================================================

#[test]
fn editor_for_every_variant_type() {
    let mappings = vec![
        (VariantType::Nil, "None"),
        (VariantType::Bool, "CheckBox"),
        (VariantType::Int, "SpinBox (Int)"),
        (VariantType::Float, "SpinBox (Float)"),
        (VariantType::String, "LineEdit"),
        (VariantType::StringName, "LineEdit"),
        (VariantType::NodePath, "NodePath"),
        (VariantType::Vector2, "Vector2"),
        (VariantType::Vector3, "Vector3"),
        (VariantType::Rect2, "Rect2"),
        (VariantType::Transform2D, "Transform2D"),
        (VariantType::Color, "ColorPicker"),
        (VariantType::Basis, "Basis"),
        (VariantType::Transform3D, "Transform3D"),
        (VariantType::Quaternion, "Quaternion"),
        (VariantType::Aabb, "AABB"),
        (VariantType::Plane, "Plane"),
        (VariantType::ObjectId, "ObjectId"),
        (VariantType::Array, "Array"),
        (VariantType::Dictionary, "Dictionary"),
        (VariantType::Callable, "Callable"),
        (VariantType::Resource, "Resource"),
    ];

    for (vtype, expected_name) in &mappings {
        let editor = PropertyEditor::for_variant_type(*vtype);
        assert_eq!(
            editor.display_name(),
            *expected_name,
            "VariantType::{vtype:?} should map to {expected_name}"
        );
    }
}

#[test]
fn editor_read_only_variants() {
    assert!(PropertyEditor::for_variant_type(VariantType::Nil).is_read_only());
    assert!(PropertyEditor::for_variant_type(VariantType::Callable).is_read_only());
    assert!(!PropertyEditor::for_variant_type(VariantType::Int).is_read_only());
    assert!(!PropertyEditor::for_variant_type(VariantType::String).is_read_only());
}

#[test]
fn editor_for_property_on_inspected_node() {
    let (tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    // position is Int → SpinBox (Int)
    let editor = panel.editor_for_property(&tree, "position");
    assert_eq!(editor.display_name(), "SpinBox (Int)");

    // velocity is Float → SpinBox (Float)
    let editor = panel.editor_for_property(&tree, "velocity");
    assert_eq!(editor.display_name(), "SpinBox (Float)");

    // visible is Bool → CheckBox
    let editor = panel.editor_for_property(&tree, "visible");
    assert_eq!(editor.display_name(), "CheckBox");

    // texture is String → LineEdit
    let editor = panel.editor_for_property(&tree, "texture");
    assert_eq!(editor.display_name(), "LineEdit");
}

#[test]
fn all_editors_returns_full_list() {
    let (tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let editors = panel.all_editors(&tree);
    assert_eq!(editors.len(), 7); // 7 properties set in make_tree_with_props
                                  // All should have non-empty display names
    for (name, editor) in &editors {
        assert!(!name.is_empty());
        assert!(!editor.display_name().is_empty());
    }
}

// ===========================================================================
// 4. EditorHint refinements
// ===========================================================================

#[test]
fn hint_range_refines_int_editor() {
    let editor = PropertyEditor::for_variant_type(VariantType::Int);
    let refined = editor.with_hint(&EditorHint::Range {
        min: 0.0,
        max: 100.0,
        step: 5.0,
    });
    match refined {
        PropertyEditor::SpinBoxInt { min, max, step } => {
            assert_eq!(min, Some(0));
            assert_eq!(max, Some(100));
            assert_eq!(step, 5);
        }
        other => panic!("expected SpinBoxInt, got {:?}", other),
    }
}

#[test]
fn hint_range_refines_float_editor() {
    let editor = PropertyEditor::for_variant_type(VariantType::Float);
    let refined = editor.with_hint(&EditorHint::Range {
        min: -1.0,
        max: 1.0,
        step: 0.1,
    });
    match refined {
        PropertyEditor::SpinBoxFloat { min, max, step } => {
            assert!((min.unwrap() - (-1.0)).abs() < f64::EPSILON);
            assert!((max.unwrap() - 1.0).abs() < f64::EPSILON);
            assert!((step - 0.1).abs() < f64::EPSILON);
        }
        other => panic!("expected SpinBoxFloat, got {:?}", other),
    }
}

#[test]
fn hint_enum_overrides_any_editor() {
    let editor = PropertyEditor::for_variant_type(VariantType::Int);
    let refined = editor.with_hint(&EditorHint::Enum(vec![
        "Low".into(),
        "Medium".into(),
        "High".into(),
    ]));
    assert_eq!(refined.display_name(), "Enum");
    match refined {
        PropertyEditor::EnumSelect { options } => {
            assert_eq!(options.len(), 3);
        }
        other => panic!("expected EnumSelect, got {:?}", other),
    }
}

#[test]
fn hint_multiline_promotes_line_to_text_edit() {
    let editor = PropertyEditor::for_variant_type(VariantType::String);
    assert_eq!(editor.display_name(), "LineEdit");
    let refined = editor.with_hint(&EditorHint::MultilineText);
    assert_eq!(refined.display_name(), "TextEdit");
}

#[test]
fn hint_multiline_does_not_affect_non_string() {
    let editor = PropertyEditor::for_variant_type(VariantType::Int);
    let refined = editor.with_hint(&EditorHint::MultilineText);
    assert_eq!(refined.display_name(), "SpinBox (Int)");
}

#[test]
fn hint_file_gives_file_picker() {
    let editor = PropertyEditor::for_variant_type(VariantType::String);
    let refined = editor.with_hint(&EditorHint::File(vec!["*.png".into(), "*.jpg".into()]));
    assert_eq!(refined.display_name(), "FilePicker");
    match refined {
        PropertyEditor::FilePicker { filters } => assert_eq!(filters.len(), 2),
        other => panic!("expected FilePicker, got {:?}", other),
    }
}

#[test]
fn hint_resource_type_gives_resource_picker() {
    let editor = PropertyEditor::for_variant_type(VariantType::Resource);
    let refined = editor.with_hint(&EditorHint::ResourceType("Texture2D".into()));
    assert_eq!(refined.display_name(), "Resource");
}

#[test]
fn hint_none_is_passthrough() {
    let editor = PropertyEditor::for_variant_type(VariantType::Bool);
    let refined = editor.clone().with_hint(&EditorHint::None);
    assert_eq!(refined, editor);
}

// ===========================================================================
// 5. Validation and coercion
// ===========================================================================

#[test]
fn validate_matching_type_ok() {
    assert!(validate_variant(&Variant::Int(42), VariantType::Int).is_ok());
    assert!(validate_variant(&Variant::Bool(true), VariantType::Bool).is_ok());
    assert!(validate_variant(&Variant::Float(1.0), VariantType::Float).is_ok());
    assert!(validate_variant(&Variant::String("hi".into()), VariantType::String).is_ok());
}

#[test]
fn validate_mismatched_type_err() {
    let result = validate_variant(&Variant::Int(42), VariantType::String);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("expected"));
}

#[test]
fn coerce_int_to_float() {
    let result = coerce_variant(&Variant::Int(42), VariantType::Float);
    assert_eq!(result, Some(Variant::Float(42.0)));
}

#[test]
fn coerce_float_to_int() {
    let result = coerce_variant(&Variant::Float(3.7), VariantType::Int);
    assert_eq!(result, Some(Variant::Int(3)));
}

#[test]
fn coerce_int_to_bool() {
    assert_eq!(
        coerce_variant(&Variant::Int(0), VariantType::Bool),
        Some(Variant::Bool(false))
    );
    assert_eq!(
        coerce_variant(&Variant::Int(1), VariantType::Bool),
        Some(Variant::Bool(true))
    );
    assert_eq!(
        coerce_variant(&Variant::Int(-5), VariantType::Bool),
        Some(Variant::Bool(true))
    );
}

#[test]
fn coerce_bool_to_int() {
    assert_eq!(
        coerce_variant(&Variant::Bool(true), VariantType::Int),
        Some(Variant::Int(1))
    );
    assert_eq!(
        coerce_variant(&Variant::Bool(false), VariantType::Int),
        Some(Variant::Int(0))
    );
}

#[test]
fn coerce_numeric_to_string() {
    assert_eq!(
        coerce_variant(&Variant::Int(42), VariantType::String),
        Some(Variant::String("42".into()))
    );
    assert_eq!(
        coerce_variant(&Variant::Bool(true), VariantType::String),
        Some(Variant::String("true".into()))
    );
}

#[test]
fn coerce_same_type_is_identity() {
    let v = Variant::Int(7);
    assert_eq!(coerce_variant(&v, VariantType::Int), Some(v));
}

#[test]
fn coerce_incompatible_returns_none() {
    // Vector2 → Bool is not supported
    assert!(coerce_variant(&Variant::Nil, VariantType::Bool).is_none());
}

// ===========================================================================
// 6. SectionedInspector — grouping and collapse
// ===========================================================================

#[test]
fn sectioned_inspector_from_entries() {
    let entries = vec![
        PropertyEntry {
            name: "position".into(),
            value: Variant::Int(0),
            category: PropertyCategory::Transform,
        },
        PropertyEntry {
            name: "visible".into(),
            value: Variant::Bool(true),
            category: PropertyCategory::Rendering,
        },
        PropertyEntry {
            name: "velocity".into(),
            value: Variant::Float(1.0),
            category: PropertyCategory::Physics,
        },
    ];

    let si = SectionedInspector::from_entries(entries);
    assert_eq!(si.section_count(), 3);
    assert_eq!(si.total_property_count(), 3);

    // Sections should be sorted: Transform, Rendering, Physics
    let names: Vec<&str> = si.sections().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Transform", "Rendering", "Physics"]);
}

#[test]
fn sectioned_inspector_collapse_expand() {
    let entries = vec![
        PropertyEntry {
            name: "position".into(),
            value: Variant::Int(0),
            category: PropertyCategory::Transform,
        },
        PropertyEntry {
            name: "visible".into(),
            value: Variant::Bool(true),
            category: PropertyCategory::Rendering,
        },
    ];

    let mut si = SectionedInspector::from_entries(entries);
    // All expanded by default
    assert_eq!(si.visible_property_count(), 2);

    si.collapse_all();
    assert_eq!(si.visible_property_count(), 0);
    assert_eq!(si.total_property_count(), 2); // still 2 total

    si.expand_all();
    assert_eq!(si.visible_property_count(), 2);
}

#[test]
fn sectioned_inspector_toggle_section() {
    let entries = vec![
        PropertyEntry {
            name: "position".into(),
            value: Variant::Int(0),
            category: PropertyCategory::Transform,
        },
        PropertyEntry {
            name: "rotation".into(),
            value: Variant::Float(0.0),
            category: PropertyCategory::Transform,
        },
        PropertyEntry {
            name: "visible".into(),
            value: Variant::Bool(true),
            category: PropertyCategory::Rendering,
        },
    ];

    let mut si = SectionedInspector::from_entries(entries);
    assert_eq!(si.visible_property_count(), 3);

    // Collapse Transform section
    let transform = si
        .section_by_category_mut(&PropertyCategory::Transform)
        .unwrap();
    assert!(transform.is_expanded());
    transform.toggle();
    assert!(!transform.is_expanded());

    assert_eq!(si.visible_property_count(), 1); // only Rendering visible
}

#[test]
fn sectioned_inspector_find_by_category() {
    let entries = vec![PropertyEntry {
        name: "mass".into(),
        value: Variant::Float(10.0),
        category: PropertyCategory::Physics,
    }];

    let si = SectionedInspector::from_entries(entries);
    assert!(si.section_by_category(&PropertyCategory::Physics).is_some());
    assert!(si
        .section_by_category(&PropertyCategory::Transform)
        .is_none());
}

#[test]
fn inspector_sectioned_view_integration() {
    let (tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let si = panel.sectioned_view(&tree);
    assert!(si.section_count() > 0);
    assert_eq!(si.total_property_count(), 7);
}

#[test]
fn inspector_section_properties() {
    let mut section = InspectorSection::new("Transform", PropertyCategory::Transform);
    assert!(section.is_empty());
    assert_eq!(section.property_count(), 0);
    assert!(section.is_expanded());

    section.set_expanded(false);
    assert!(!section.is_expanded());
}

// ===========================================================================
// 7. ResourceSubEditor
// ===========================================================================

#[test]
fn resource_sub_editor_list_properties() {
    let res = make_resource(
        "Texture2D",
        vec![
            ("width", Variant::Int(256)),
            ("height", Variant::Int(256)),
            ("filter", Variant::Bool(true)),
        ],
    );
    let sub = ResourceSubEditor::new(res);

    let props = sub.list_properties();
    assert_eq!(props.len(), 3);
    // Each should have an editor
    for p in &props {
        assert!(!p.editor.display_name().is_empty());
    }
}

#[test]
fn resource_sub_editor_get_set() {
    let res = make_resource("Material", vec![("albedo", Variant::String("red".into()))]);
    let mut sub = ResourceSubEditor::new(res);

    assert_eq!(
        sub.get_property("albedo"),
        Some(&Variant::String("red".into()))
    );
    sub.set_property("albedo", Variant::String("blue".into()));
    assert_eq!(
        sub.get_property("albedo"),
        Some(&Variant::String("blue".into()))
    );
}

#[test]
fn resource_sub_editor_change_log() {
    let res = make_resource("Material", vec![("color", Variant::String("white".into()))]);
    let mut sub = ResourceSubEditor::new(res);
    assert_eq!(sub.change_count(), 0);

    sub.set_property("color", Variant::String("black".into()));
    assert_eq!(sub.change_count(), 1);

    let log = sub.change_log();
    assert_eq!(log[0].property, "color");
    assert_eq!(log[0].old_value, Variant::String("white".into()));
    assert_eq!(log[0].new_value, Variant::String("black".into()));
}

#[test]
fn resource_sub_editor_undo() {
    let res = make_resource("Material", vec![("alpha", Variant::Float(1.0))]);
    let mut sub = ResourceSubEditor::new(res);

    sub.set_property("alpha", Variant::Float(0.5));
    assert_eq!(sub.get_property("alpha"), Some(&Variant::Float(0.5)));

    let undone = sub.undo_last();
    assert_eq!(undone, Some("alpha".to_string()));
    assert_eq!(sub.get_property("alpha"), Some(&Variant::Float(1.0)));
    assert_eq!(sub.change_count(), 0);
}

#[test]
fn resource_sub_editor_expand_collapse() {
    let res = make_resource("Texture2D", vec![]);
    let mut sub = ResourceSubEditor::new(res);

    assert!(!sub.is_expanded());
    sub.toggle();
    assert!(sub.is_expanded());
    sub.set_expanded(false);
    assert!(!sub.is_expanded());
}

#[test]
fn resource_sub_editor_replace() {
    let res1 = make_resource("Mat1", vec![("a", Variant::Int(1))]);
    let res2 = make_resource("Mat2", vec![("b", Variant::Int(2))]);
    let mut sub = ResourceSubEditor::new(res1);

    sub.set_property("a", Variant::Int(10));
    assert_eq!(sub.change_count(), 1);

    sub.replace_resource(res2);
    assert_eq!(sub.class_name(), "Mat2");
    assert_eq!(sub.change_count(), 0); // log cleared
}

#[test]
fn resource_sub_editor_class_and_path() {
    let mut res = Resource::new("ShaderMaterial");
    res.path = "res://materials/shader.tres".into();
    let sub = ResourceSubEditor::new(Arc::new(res));

    assert_eq!(sub.class_name(), "ShaderMaterial");
    assert_eq!(sub.resource_path(), "res://materials/shader.tres");
}

// ===========================================================================
// 8. set_property_validated
// ===========================================================================

#[test]
fn validated_set_same_type_ok() {
    let (mut tree, node_id) = make_tree_with_props();
    let mut p = InspectorPanel::new();
    p.inspect(node_id);

    let result = p.set_property_validated(&mut tree, "position", Variant::Int(99));
    assert!(result.is_ok());
    assert_eq!(p.get_property(&tree, "position"), Variant::Int(99));
}

#[test]
fn validated_set_coerces_float_to_int() {
    let (mut tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    // position is Int, setting Float should coerce
    let result = panel.set_property_validated(&mut tree, "position", Variant::Float(7.9));
    assert!(result.is_ok());
    assert_eq!(panel.get_property(&tree, "position"), Variant::Int(7));
}

#[test]
fn validated_set_rejects_incompatible() {
    let (mut tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    // visible is Bool, setting Array should fail
    let result = panel.set_property_validated(&mut tree, "visible", Variant::Array(vec![]));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("expected"));
}

#[test]
fn validated_set_nil_property_accepts_anything() {
    let (mut tree, node_id) = make_tree_with_props();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    // custom_data is Nil — should accept any type
    let result =
        panel.set_property_validated(&mut tree, "custom_data", Variant::String("hello".into()));
    assert!(result.is_ok());
}

// ===========================================================================
// 9. Edge cases
// ===========================================================================

#[test]
fn editor_default_spinbox_int_step_is_one() {
    match PropertyEditor::for_variant_type(VariantType::Int) {
        PropertyEditor::SpinBoxInt { step, .. } => assert_eq!(step, 1),
        other => panic!("expected SpinBoxInt, got {:?}", other),
    }
}

#[test]
fn editor_default_spinbox_float_step_is_small() {
    match PropertyEditor::for_variant_type(VariantType::Float) {
        PropertyEditor::SpinBoxFloat { step, .. } => assert!((step - 0.001).abs() < f64::EPSILON),
        other => panic!("expected SpinBoxFloat, got {:?}", other),
    }
}

#[test]
fn hint_range_step_floor_for_int() {
    // If step < 1 for int, it should clamp to 1
    let editor = PropertyEditor::for_variant_type(VariantType::Int);
    let refined = editor.with_hint(&EditorHint::Range {
        min: 0.0,
        max: 10.0,
        step: 0.5,
    });
    match refined {
        PropertyEditor::SpinBoxInt { step, .. } => assert_eq!(step, 1),
        other => panic!("expected SpinBoxInt, got {:?}", other),
    }
}

#[test]
fn coerce_string_to_node_path() {
    let result = coerce_variant(
        &Variant::String("root/Player".into()),
        VariantType::NodePath,
    );
    assert!(result.is_some());
    match result.unwrap() {
        Variant::NodePath(np) => assert_eq!(np.to_string(), "root/Player"),
        other => panic!("expected NodePath, got {:?}", other),
    }
}

#[test]
fn display_names_all_nonempty() {
    let all_types = [
        VariantType::Nil,
        VariantType::Bool,
        VariantType::Int,
        VariantType::Float,
        VariantType::String,
        VariantType::StringName,
        VariantType::NodePath,
        VariantType::Vector2,
        VariantType::Vector3,
        VariantType::Rect2,
        VariantType::Transform2D,
        VariantType::Color,
        VariantType::Basis,
        VariantType::Transform3D,
        VariantType::Quaternion,
        VariantType::Aabb,
        VariantType::Plane,
        VariantType::ObjectId,
        VariantType::Array,
        VariantType::Dictionary,
        VariantType::Callable,
        VariantType::Resource,
    ];
    for vtype in &all_types {
        let editor = PropertyEditor::for_variant_type(*vtype);
        assert!(
            !editor.display_name().is_empty(),
            "{vtype:?} has empty display name"
        );
    }
}
