//! Inspector → Control layout binding.
//!
//! Routes property edits made through the [`InspectorPanel`] back into the
//! `gdscene::control` layout solver so anchor/margin/size mutations reflow
//! the rect immediately. Mirrors Godot's behavior where moving an anchor in
//! the inspector instantly retargets the on-screen control.

use gdcore::math::Vector2;
use gdscene::node::NodeId;
use gdscene::SceneTree;
use gdvariant::Variant;

use crate::inspector::InspectorPanel;

/// Property names whose mutation invalidates a Control's layout. Matches the
/// inputs read by [`gdscene::control::resolve_control_layout`] and
/// [`gdscene::control::arrange_container`].
fn is_layout_property(name: &str) -> bool {
    matches!(
        name,
        "anchor_left"
            | "anchor_top"
            | "anchor_right"
            | "anchor_bottom"
            | "offset_left"
            | "offset_top"
            | "offset_right"
            | "offset_bottom"
            | "size_flags_horizontal"
            | "size_flags_vertical"
            | "custom_minimum_size"
            | "min_size"
            | "grow_horizontal"
            | "grow_vertical"
            | "theme_override_constants/separation"
            | "columns"
    )
}

fn is_container_class(class: &str) -> bool {
    matches!(class, "VBoxContainer" | "HBoxContainer" | "GridContainer")
}

/// Re-runs the layout solver for `node_id` after a property change.
///
/// - Re-resolves the node's own anchors/margins against `parent_size`.
/// - If the node itself is a container, re-arranges its children.
/// - If the node's parent is a container, re-arranges the parent so the
///   node's new minimum size / size flags propagate to siblings.
pub fn invalidate_layout(tree: &mut SceneTree, node_id: NodeId, parent_size: Vector2) {
    gdscene::control::resolve_control_layout(tree, node_id, parent_size);

    let class = tree
        .get_node(node_id)
        .map(|n| n.class_name().to_string())
        .unwrap_or_default();
    if is_container_class(&class) {
        let total = gdscene::control::get_size(tree, node_id);
        gdscene::control::arrange_container(tree, node_id, total);
    }

    if let Some(parent_id) = tree.get_node(node_id).and_then(|n| n.parent()) {
        let parent_class = tree
            .get_node(parent_id)
            .map(|n| n.class_name().to_string())
            .unwrap_or_default();
        if is_container_class(&parent_class) {
            let parent_total = gdscene::control::get_size(tree, parent_id);
            gdscene::control::arrange_container(tree, parent_id, parent_total);
        }
    }
}

/// Sets a property on the inspected Control via [`InspectorPanel::set_property`]
/// and, when the property is layout-relevant, re-runs the layout solver so
/// the node's rect (and any container children / siblings) reflect the edit.
///
/// Returns the previous value of the property, exactly like
/// [`InspectorPanel::set_property`].
pub fn set_control_property_with_layout(
    inspector: &InspectorPanel,
    tree: &mut SceneTree,
    name: &str,
    value: Variant,
    parent_size: Vector2,
) -> Variant {
    let old = inspector.set_property(tree, name, value);
    if is_layout_property(name) {
        if let Some(node_id) = inspector.inspected_node() {
            invalidate_layout(tree, node_id, parent_size);
        }
    }
    old
}
