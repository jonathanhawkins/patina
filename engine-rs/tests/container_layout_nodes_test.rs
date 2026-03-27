//! pat-grdja: ScrollContainer, TabContainer, and SplitContainer layout nodes.
//!
//! Validates that the container layout nodes:
//! - Can be created and added to the scene tree
//! - Support all relevant properties via control module helpers
//! - Load correctly from .tscn fixtures
//! - Work in typical UI hierarchies

use gdscene::control;
use gdscene::node::Node;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

fn add_control(tree: &mut SceneTree, parent: gdscene::node::NodeId, name: &str, class: &str) -> gdscene::node::NodeId {
    let node = Node::new(name, class);
    tree.add_child(parent, node).unwrap()
}

// ===========================================================================
// 1. ScrollContainer
// ===========================================================================

#[test]
fn scroll_container_in_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sc = add_control(&mut tree, root, "Scroll", "ScrollContainer");
    assert_eq!(tree.get_node(sc).unwrap().class_name(), "ScrollContainer");
}

#[test]
fn scroll_container_horizontal_mode() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sc = add_control(&mut tree, root, "Scroll", "ScrollContainer");

    control::set_horizontal_scroll_mode(&mut tree, sc, control::ScrollMode::NeverShow);
    assert_eq!(control::get_horizontal_scroll_mode(&tree, sc), control::ScrollMode::NeverShow as i64);
}

#[test]
fn scroll_container_vertical_mode() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sc = add_control(&mut tree, root, "Scroll", "ScrollContainer");

    control::set_vertical_scroll_mode(&mut tree, sc, control::ScrollMode::AlwaysShow);
    assert_eq!(control::get_vertical_scroll_mode(&tree, sc), control::ScrollMode::AlwaysShow as i64);
}

#[test]
fn scroll_container_deadzone() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sc = add_control(&mut tree, root, "Scroll", "ScrollContainer");

    control::set_scroll_deadzone(&mut tree, sc, 10);
    assert_eq!(control::get_scroll_deadzone(&tree, sc), 10);
}

#[test]
fn scroll_container_with_child_content() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let sc = add_control(&mut tree, root, "Scroll", "ScrollContainer");
    let vbox = add_control(&mut tree, sc, "Content", "VBoxContainer");

    for i in 0..5 {
        add_control(&mut tree, vbox, &format!("Item{i}"), "Label");
    }

    assert_eq!(tree.get_node(sc).unwrap().children().len(), 1);
    assert_eq!(tree.get_node(vbox).unwrap().children().len(), 5);
}

// ===========================================================================
// 2. TabContainer
// ===========================================================================

#[test]
fn tab_container_in_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tc = add_control(&mut tree, root, "Tabs", "TabContainer");
    assert_eq!(tree.get_node(tc).unwrap().class_name(), "TabContainer");
}

#[test]
fn tab_container_current_tab() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tc = add_control(&mut tree, root, "Tabs", "TabContainer");

    // Default is tab 0.
    assert_eq!(control::get_current_tab(&tree, tc), 0);

    control::set_current_tab(&mut tree, tc, 2);
    assert_eq!(control::get_current_tab(&tree, tc), 2);
}

#[test]
fn tab_container_alignment() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tc = add_control(&mut tree, root, "Tabs", "TabContainer");

    control::set_tab_alignment(&mut tree, tc, control::TabAlignment::Center);
    assert_eq!(control::get_tab_alignment(&tree, tc), control::TabAlignment::Center as i64);
}

#[test]
fn tab_container_drag_rearrange() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tc = add_control(&mut tree, root, "Tabs", "TabContainer");

    assert!(!control::get_drag_to_rearrange_enabled(&tree, tc));
    control::set_drag_to_rearrange_enabled(&mut tree, tc, true);
    assert!(control::get_drag_to_rearrange_enabled(&tree, tc));
}

#[test]
fn tab_container_with_tab_children() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let tc = add_control(&mut tree, root, "Tabs", "TabContainer");

    add_control(&mut tree, tc, "General", "Panel");
    add_control(&mut tree, tc, "Advanced", "Panel");
    add_control(&mut tree, tc, "Debug", "Panel");

    assert_eq!(tree.get_node(tc).unwrap().children().len(), 3);
}

// ===========================================================================
// 3. SplitContainer (HSplitContainer / VSplitContainer)
// ===========================================================================

#[test]
fn hsplit_container_in_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let hs = add_control(&mut tree, root, "HSplit", "HSplitContainer");
    assert_eq!(tree.get_node(hs).unwrap().class_name(), "HSplitContainer");
}

#[test]
fn vsplit_container_in_tree() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let vs = add_control(&mut tree, root, "VSplit", "VSplitContainer");
    assert_eq!(tree.get_node(vs).unwrap().class_name(), "VSplitContainer");
}

#[test]
fn split_container_offset() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let hs = add_control(&mut tree, root, "Split", "HSplitContainer");

    control::set_split_offset(&mut tree, hs, 200);
    assert_eq!(control::get_split_offset(&tree, hs), 200);
}

#[test]
fn split_container_collapsed() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let vs = add_control(&mut tree, root, "Split", "VSplitContainer");

    assert!(!control::get_collapsed(&tree, vs));
    control::set_collapsed(&mut tree, vs, true);
    assert!(control::get_collapsed(&tree, vs));
}

#[test]
fn split_container_dragger_visibility() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let hs = add_control(&mut tree, root, "Split", "HSplitContainer");

    control::set_dragger_visibility(&mut tree, hs, control::DraggerVisibility::Hidden);
    assert_eq!(
        control::get_dragger_visibility(&tree, hs),
        control::DraggerVisibility::Hidden as i64
    );
}

#[test]
fn split_container_with_two_panels() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let hs = add_control(&mut tree, root, "Split", "HSplitContainer");

    let left = add_control(&mut tree, hs, "Left", "Panel");
    let right = add_control(&mut tree, hs, "Right", "Panel");

    assert_eq!(tree.get_node(hs).unwrap().children().len(), 2);
    assert_eq!(tree.get_node(left).unwrap().parent(), Some(hs));
    assert_eq!(tree.get_node(right).unwrap().parent(), Some(hs));
}

// ===========================================================================
// 4. Packed scene loading with containers
// ===========================================================================

#[test]
fn containers_from_tscn() {
    let tscn = r#"[gd_scene format=3 uid="uid://container_test"]

[node name="UI" type="Control"]

[node name="Tabs" type="TabContainer" parent="."]
current_tab = 1

[node name="General" type="Panel" parent="Tabs"]

[node name="Settings" type="Panel" parent="Tabs"]

[node name="Scroll" type="ScrollContainer" parent="."]

[node name="List" type="VBoxContainer" parent="Scroll"]

[node name="Split" type="HSplitContainer" parent="."]
split_offset = 150

[node name="Left" type="Panel" parent="Split"]

[node name="Right" type="Panel" parent="Split"]
"#;
    let packed = gdscene::packed_scene::PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree(&mut tree, root, &packed).unwrap();

    // Verify all container nodes loaded.
    assert!(tree.get_node_by_path("/root/UI/Tabs").is_some());
    assert!(tree.get_node_by_path("/root/UI/Scroll").is_some());
    assert!(tree.get_node_by_path("/root/UI/Split").is_some());

    // Verify TabContainer property.
    let tabs = tree.get_node_by_path("/root/UI/Tabs").unwrap();
    assert_eq!(control::get_current_tab(&tree, tabs), 1);

    // Verify SplitContainer property.
    let split = tree.get_node_by_path("/root/UI/Split").unwrap();
    assert_eq!(control::get_split_offset(&tree, split), 150);

    // Verify hierarchy.
    assert_eq!(tree.get_node(tabs).unwrap().children().len(), 2);
    assert_eq!(tree.get_node(split).unwrap().children().len(), 2);
}

// ===========================================================================
// 5. Nested container hierarchy
// ===========================================================================

#[test]
fn nested_containers() {
    let mut tree = SceneTree::new();
    let root = tree.root_id();

    let hsplit = add_control(&mut tree, root, "Main", "HSplitContainer");
    let left_scroll = add_control(&mut tree, hsplit, "LeftScroll", "ScrollContainer");
    let right_tabs = add_control(&mut tree, hsplit, "RightTabs", "TabContainer");

    let list = add_control(&mut tree, left_scroll, "List", "VBoxContainer");
    for i in 0..3 {
        add_control(&mut tree, list, &format!("Item{i}"), "Label");
    }

    add_control(&mut tree, right_tabs, "Tab1", "Panel");
    add_control(&mut tree, right_tabs, "Tab2", "Panel");

    // Verify structure.
    assert_eq!(tree.get_node(hsplit).unwrap().children().len(), 2);
    assert_eq!(tree.get_node(left_scroll).unwrap().children().len(), 1);
    assert_eq!(tree.get_node(list).unwrap().children().len(), 3);
    assert_eq!(tree.get_node(right_tabs).unwrap().children().len(), 2);

    // Set properties on nested containers.
    control::set_split_offset(&mut tree, hsplit, 300);
    control::set_current_tab(&mut tree, right_tabs, 1);
    control::set_vertical_scroll_mode(&mut tree, left_scroll, control::ScrollMode::AlwaysShow);

    assert_eq!(control::get_split_offset(&tree, hsplit), 300);
    assert_eq!(control::get_current_tab(&tree, right_tabs), 1);
    assert_eq!(
        control::get_vertical_scroll_mode(&tree, left_scroll),
        control::ScrollMode::AlwaysShow as i64
    );
}
