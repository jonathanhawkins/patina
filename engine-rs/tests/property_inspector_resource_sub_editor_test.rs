//! pat-x0mi5: Property inspector resource sub-editor (inline resource editing).
//!
//! Validates:
//! 1. ResourceSubEditor creation from Arc<Resource>
//! 2. Class name and resource path accessors
//! 3. Expand/collapse toggle and set_expanded
//! 4. List properties with correct editors
//! 5. Get and set properties (clone-on-write)
//! 6. Change log recording and undo support
//! 7. Multiple changes with sequential undo
//! 8. Replace resource clears change log
//! 9. Subresource navigation (nested inline resources)
//! 10. Non-existent subresource returns None
//! 11. Property count accessor
//! 12. Editor types match variant types
//! 13. InspectorPanel integration with resource properties
//! 14. SectionedInspector with resource properties
//! 15. PropertyEditor for resource type returns ResourcePicker

use gdeditor::inspector::{
    InspectorPanel, InspectorSection, PropertyCategory, PropertyEditor, ResourceSubEditor,
    SectionedInspector,
};
use gdresource::Resource;
use gdscene::node::{Node, NodeId};
use gdscene::SceneTree;
use gdvariant::{Variant, VariantType};
use std::sync::Arc;

// ── Helpers ─────────────────────────────────────────────────────────

fn make_resource() -> Arc<Resource> {
    let mut res = Resource::new("StyleBoxFlat");
    res.path = "res://theme.tres".to_string();
    res.set_property("bg_color", Variant::String("red".into()));
    res.set_property("border_width", Variant::Int(2));
    res.set_property("corner_radius", Variant::Float(4.0));
    Arc::new(res)
}

fn make_tree_with_node() -> (SceneTree, NodeId) {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut node = Node::new("Player", "Node2D");
    node.set_property("position", Variant::Int(0));
    node.set_property("velocity", Variant::Float(1.5));
    node.set_property("visible", Variant::Bool(true));
    let id = tree.add_child(root, node).unwrap();
    (tree, id)
}

// ── ResourceSubEditor creation ──────────────────────────────────────

#[test]
fn sub_editor_creation() {
    let res = make_resource();
    let editor = ResourceSubEditor::new(res.clone());
    assert_eq!(editor.class_name(), "StyleBoxFlat");
    assert_eq!(editor.resource_path(), "res://theme.tres");
    assert!(!editor.is_expanded());
    assert_eq!(editor.change_count(), 0);
    assert_eq!(editor.property_count(), 3);
}

// ── Expand/collapse ─────────────────────────────────────────────────

#[test]
fn sub_editor_toggle() {
    let mut editor = ResourceSubEditor::new(make_resource());
    assert!(!editor.is_expanded());
    editor.toggle();
    assert!(editor.is_expanded());
    editor.toggle();
    assert!(!editor.is_expanded());
}

#[test]
fn sub_editor_set_expanded() {
    let mut editor = ResourceSubEditor::new(make_resource());
    editor.set_expanded(true);
    assert!(editor.is_expanded());
    editor.set_expanded(false);
    assert!(!editor.is_expanded());
}

// ── List properties ─────────────────────────────────────────────────

#[test]
fn sub_editor_list_properties_sorted() {
    let editor = ResourceSubEditor::new(make_resource());
    let props = editor.list_properties();
    assert_eq!(props.len(), 3);
    assert_eq!(props[0].name, "bg_color");
    assert_eq!(props[1].name, "border_width");
    assert_eq!(props[2].name, "corner_radius");
}

#[test]
fn sub_editor_property_editors_match_types() {
    let editor = ResourceSubEditor::new(make_resource());
    let props = editor.list_properties();

    let bg = props.iter().find(|p| p.name == "bg_color").unwrap();
    assert_eq!(bg.editor, PropertyEditor::LineEdit);

    let border = props.iter().find(|p| p.name == "border_width").unwrap();
    assert!(matches!(border.editor, PropertyEditor::SpinBoxInt { .. }));

    let radius = props.iter().find(|p| p.name == "corner_radius").unwrap();
    assert!(matches!(radius.editor, PropertyEditor::SpinBoxFloat { .. }));
}

// ── Get/set properties ──────────────────────────────────────────────

#[test]
fn sub_editor_get_property() {
    let editor = ResourceSubEditor::new(make_resource());
    assert_eq!(editor.get_property("border_width"), Some(&Variant::Int(2)));
    assert!(editor.get_property("nonexistent").is_none());
}

#[test]
fn sub_editor_set_property_clone_on_write() {
    let res = make_resource();
    let mut editor = ResourceSubEditor::new(res.clone());

    let new_res = editor.set_property("border_width", Variant::Int(10));
    // New resource has updated value
    assert_eq!(
        new_res.get_property("border_width"),
        Some(&Variant::Int(10))
    );
    // Editor's internal resource also updated
    assert_eq!(editor.get_property("border_width"), Some(&Variant::Int(10)));
    // Original resource unchanged (Arc clone-on-write)
    assert_eq!(res.get_property("border_width"), Some(&Variant::Int(2)));
}

// ── Change log and undo ─────────────────────────────────────────────

#[test]
fn sub_editor_change_log() {
    let mut editor = ResourceSubEditor::new(make_resource());
    editor.set_property("border_width", Variant::Int(5));

    assert_eq!(editor.change_count(), 1);
    let change = &editor.change_log()[0];
    assert_eq!(change.property, "border_width");
    assert_eq!(change.old_value, Variant::Int(2));
    assert_eq!(change.new_value, Variant::Int(5));
}

#[test]
fn sub_editor_undo_last() {
    let mut editor = ResourceSubEditor::new(make_resource());
    editor.set_property("border_width", Variant::Int(10));

    let undone = editor.undo_last();
    assert_eq!(undone, Some("border_width".to_string()));
    assert_eq!(editor.get_property("border_width"), Some(&Variant::Int(2)));
    assert_eq!(editor.change_count(), 0);
}

#[test]
fn sub_editor_undo_empty_returns_none() {
    let mut editor = ResourceSubEditor::new(make_resource());
    assert!(editor.undo_last().is_none());
}

#[test]
fn sub_editor_multiple_changes_sequential_undo() {
    let mut editor = ResourceSubEditor::new(make_resource());

    editor.set_property("border_width", Variant::Int(5));
    editor.set_property("corner_radius", Variant::Float(8.0));
    editor.set_property("bg_color", Variant::String("blue".into()));
    assert_eq!(editor.change_count(), 3);

    // Undo bg_color
    editor.undo_last();
    assert_eq!(
        editor.get_property("bg_color"),
        Some(&Variant::String("red".into()))
    );
    assert_eq!(editor.change_count(), 2);

    // Undo corner_radius
    editor.undo_last();
    assert_eq!(
        editor.get_property("corner_radius"),
        Some(&Variant::Float(4.0))
    );
    assert_eq!(editor.change_count(), 1);

    // Undo border_width
    editor.undo_last();
    assert_eq!(editor.get_property("border_width"), Some(&Variant::Int(2)));
    assert_eq!(editor.change_count(), 0);
}

// ── Replace resource ────────────────────────────────────────────────

#[test]
fn sub_editor_replace_resource() {
    let mut editor = ResourceSubEditor::new(make_resource());
    editor.set_property("border_width", Variant::Int(99));
    assert_eq!(editor.change_count(), 1);

    let mut new_res = Resource::new("GradientTexture");
    new_res.set_property("width", Variant::Int(256));
    editor.replace_resource(Arc::new(new_res));

    assert_eq!(editor.class_name(), "GradientTexture");
    assert_eq!(editor.change_count(), 0);
    assert_eq!(editor.get_property("width"), Some(&Variant::Int(256)));
}

// ── Subresource navigation ──────────────────────────────────────────

#[test]
fn sub_editor_subresource_navigation() {
    let mut res = Resource::new("Theme");
    res.set_property("name", Variant::String("MyTheme".into()));

    let mut sub = Resource::new("StyleBoxFlat");
    sub.set_property("bg_color", Variant::String("green".into()));
    res.subresources
        .insert("StyleBoxFlat_001".into(), Arc::new(sub));

    let editor = ResourceSubEditor::new(Arc::new(res));
    let ids = editor.subresource_ids();
    assert_eq!(ids.len(), 1);
    assert!(ids.contains(&"StyleBoxFlat_001".to_string()));

    let sub_editor = editor.open_subresource("StyleBoxFlat_001").unwrap();
    assert_eq!(sub_editor.class_name(), "StyleBoxFlat");
    assert_eq!(
        sub_editor.get_property("bg_color"),
        Some(&Variant::String("green".into()))
    );
}

#[test]
fn sub_editor_nonexistent_subresource() {
    let editor = ResourceSubEditor::new(make_resource());
    assert!(editor.open_subresource("nope").is_none());
}

#[test]
fn sub_editor_nested_subresource_editing() {
    let mut inner = Resource::new("GradientTexture");
    inner.set_property("width", Variant::Int(128));

    let mut outer = Resource::new("Material");
    outer.set_property("shader", Variant::String("default".into()));
    outer.subresources.insert("tex_001".into(), Arc::new(inner));

    let outer_editor = ResourceSubEditor::new(Arc::new(outer));
    let mut inner_editor = outer_editor.open_subresource("tex_001").unwrap();

    // Edit inner resource
    inner_editor.set_property("width", Variant::Int(512));
    assert_eq!(inner_editor.get_property("width"), Some(&Variant::Int(512)));
    assert_eq!(inner_editor.change_count(), 1);

    // Undo inner edit
    inner_editor.undo_last();
    assert_eq!(inner_editor.get_property("width"), Some(&Variant::Int(128)));
}

// ── PropertyEditor for Resource type ────────────────────────────────

#[test]
fn resource_variant_gets_resource_picker() {
    let editor = PropertyEditor::for_variant_type(VariantType::Resource);
    assert_eq!(editor, PropertyEditor::ResourcePicker);
    assert_eq!(editor.display_name(), "Resource");
    assert!(!editor.is_read_only());
}

// ── InspectorPanel integration ──────────────────────────────────────

#[test]
fn inspector_panel_list_and_set_properties() {
    let (mut tree, node_id) = make_tree_with_node();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let props = panel.list_properties(&tree);
    assert_eq!(props.len(), 3);

    let old = panel.set_property(&mut tree, "visible", Variant::Bool(false));
    assert_eq!(old, Variant::Bool(true));
    assert_eq!(panel.get_property(&tree, "visible"), Variant::Bool(false));
}

#[test]
fn inspector_panel_clear() {
    let mut panel = InspectorPanel::new();
    let (tree, node_id) = make_tree_with_node();
    panel.inspect(node_id);
    assert!(panel.inspected_node().is_some());
    panel.clear();
    assert!(panel.inspected_node().is_none());
    assert!(panel.list_properties(&tree).is_empty());
}

#[test]
fn inspector_panel_editor_for_property() {
    let (tree, node_id) = make_tree_with_node();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let editors = panel.all_editors(&tree);
    assert_eq!(editors.len(), 3);
    // Find the visible property (Bool -> CheckBox)
    let visible_editor = editors.iter().find(|(n, _)| n == "visible");
    assert!(visible_editor.is_some());
    assert_eq!(visible_editor.unwrap().1, PropertyEditor::CheckBox);
}

// ── SectionedInspector ──────────────────────────────────────────────

#[test]
fn sectioned_inspector_groups_by_category() {
    let (tree, node_id) = make_tree_with_node();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let entries = panel.list_properties(&tree);
    let view = SectionedInspector::from_entries(entries);
    assert_eq!(view.section_count(), 3); // Transform, Rendering, Physics
    assert_eq!(view.total_property_count(), 3);
}

#[test]
fn sectioned_inspector_collapse_expand_all() {
    let (tree, node_id) = make_tree_with_node();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let entries = panel.list_properties(&tree);
    let mut view = SectionedInspector::from_entries(entries);

    assert_eq!(view.visible_property_count(), 3);
    view.collapse_all();
    assert_eq!(view.visible_property_count(), 0);
    view.expand_all();
    assert_eq!(view.visible_property_count(), 3);
}

#[test]
fn sectioned_inspector_find_by_category() {
    let (tree, node_id) = make_tree_with_node();
    let mut panel = InspectorPanel::new();
    panel.inspect(node_id);

    let entries = panel.list_properties(&tree);
    let view = SectionedInspector::from_entries(entries);

    let transform = view.section_by_category(&PropertyCategory::Transform);
    assert!(transform.is_some());
    assert_eq!(transform.unwrap().property_count(), 1);
}

// ── InspectorSection ────────────────────────────────────────────────

#[test]
fn inspector_section_defaults() {
    let section = InspectorSection::new("Test", PropertyCategory::Misc);
    assert!(section.is_expanded());
    assert!(section.is_empty());
    assert_eq!(section.property_count(), 0);
    assert_eq!(section.name, "Test");
}

#[test]
fn inspector_section_toggle() {
    let mut section = InspectorSection::new("Test", PropertyCategory::Misc);
    assert!(section.is_expanded());
    section.toggle();
    assert!(!section.is_expanded());
    section.toggle();
    assert!(section.is_expanded());
}
