//! Group management dialog for node groups.
//!
//! Mirrors Godot's "Groups" tab in the Node dock: lets the user view,
//! add, and remove group memberships for a selected node, and browse
//! all groups currently in use across the scene tree.

use gdscene::node::NodeId;
use gdscene::SceneTree;
use std::collections::{BTreeMap, HashSet};

/// A single group entry shown in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupEntry {
    /// The group name.
    pub name: String,
    /// Number of nodes currently in this group.
    pub member_count: usize,
    /// Whether the inspected node is a member of this group.
    pub node_is_member: bool,
}

/// Result of applying a group action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupActionResult {
    /// The group that was affected.
    pub group_name: String,
    /// Human-readable description of what happened.
    pub description: String,
}

/// The group management dialog.
///
/// Displays all groups in the scene tree and which groups the currently
/// inspected node belongs to. Supports adding/removing the node from
/// groups and creating new groups.
#[derive(Debug)]
pub struct GroupDialog {
    /// The node currently being inspected.
    inspected_node: Option<NodeId>,
    /// Whether the dialog is currently open/visible.
    visible: bool,
    /// Current filter/search text.
    filter_text: String,
}

impl GroupDialog {
    /// Creates a new group dialog.
    pub fn new() -> Self {
        Self {
            inspected_node: None,
            visible: false,
            filter_text: String::new(),
        }
    }

    /// Opens the dialog for a specific node.
    pub fn open(&mut self, node_id: NodeId) {
        self.inspected_node = Some(node_id);
        self.visible = true;
        self.filter_text.clear();
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Returns whether the dialog is open.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Returns the currently inspected node, if any.
    pub fn inspected_node(&self) -> Option<NodeId> {
        self.inspected_node
    }

    /// Sets the inspected node without opening/closing the dialog.
    pub fn set_inspected_node(&mut self, node_id: Option<NodeId>) {
        self.inspected_node = node_id;
    }

    /// Sets the filter text for narrowing the group list.
    pub fn set_filter(&mut self, text: impl Into<String>) {
        self.filter_text = text.into();
    }

    /// Returns the current filter text.
    pub fn filter_text(&self) -> &str {
        &self.filter_text
    }

    /// Collects all groups across the entire scene tree.
    ///
    /// Returns a sorted map of group name -> set of member NodeIds.
    pub fn collect_all_groups(&self, tree: &SceneTree) -> BTreeMap<String, HashSet<NodeId>> {
        let mut groups: BTreeMap<String, HashSet<NodeId>> = BTreeMap::new();
        Self::walk_groups(tree, tree.root_id(), &mut groups);
        groups
    }

    /// Recursive helper to collect groups from all nodes.
    fn walk_groups(
        tree: &SceneTree,
        id: NodeId,
        groups: &mut BTreeMap<String, HashSet<NodeId>>,
    ) {
        let node = match tree.get_node(id) {
            Some(n) => n,
            None => return,
        };
        let children: Vec<NodeId> = node.children().to_vec();
        for group in node.groups() {
            groups.entry(group.clone()).or_default().insert(id);
        }
        for child_id in children {
            Self::walk_groups(tree, child_id, groups);
        }
    }

    /// Returns all groups as entries, filtered by the current filter text.
    ///
    /// Each entry indicates the group name, member count, and whether
    /// the inspected node is a member.
    pub fn filtered_groups(&self, tree: &SceneTree) -> Vec<GroupEntry> {
        let all_groups = self.collect_all_groups(tree);
        let filter_lower = self.filter_text.to_lowercase();

        all_groups
            .into_iter()
            .filter(|(name, _)| {
                filter_lower.is_empty() || name.to_lowercase().contains(&filter_lower)
            })
            .map(|(name, members)| {
                let node_is_member = self
                    .inspected_node
                    .map(|nid| members.contains(&nid))
                    .unwrap_or(false);
                GroupEntry {
                    name,
                    member_count: members.len(),
                    node_is_member,
                }
            })
            .collect()
    }

    /// Returns groups that the inspected node belongs to.
    pub fn node_groups(&self, tree: &SceneTree) -> Vec<String> {
        let node_id = match self.inspected_node {
            Some(id) => id,
            None => return Vec::new(),
        };
        let node = match tree.get_node(node_id) {
            Some(n) => n,
            None => return Vec::new(),
        };
        let mut groups: Vec<String> = node.groups().iter().cloned().collect();
        groups.sort();
        groups
    }

    /// Adds the inspected node to a group.
    ///
    /// Returns an error if no node is inspected or the node doesn't exist.
    pub fn add_to_group(
        &self,
        tree: &mut SceneTree,
        group: &str,
    ) -> Result<GroupActionResult, String> {
        let node_id = self
            .inspected_node
            .ok_or_else(|| "no node inspected".to_string())?;

        if group.is_empty() {
            return Err("group name cannot be empty".to_string());
        }

        tree.add_to_group(node_id, group)
            .map_err(|e| e.to_string())?;

        Ok(GroupActionResult {
            group_name: group.to_string(),
            description: format!("Added node to group \"{group}\""),
        })
    }

    /// Removes the inspected node from a group.
    pub fn remove_from_group(
        &self,
        tree: &mut SceneTree,
        group: &str,
    ) -> Result<GroupActionResult, String> {
        let node_id = self
            .inspected_node
            .ok_or_else(|| "no node inspected".to_string())?;

        tree.remove_from_group(node_id, group)
            .map_err(|e| e.to_string())?;

        Ok(GroupActionResult {
            group_name: group.to_string(),
            description: format!("Removed node from group \"{group}\""),
        })
    }

    /// Adds a new group to a node (convenience for creating a group
    /// that didn't exist before — semantically the same as `add_to_group`).
    pub fn create_group(
        &self,
        tree: &mut SceneTree,
        group: &str,
    ) -> Result<GroupActionResult, String> {
        self.add_to_group(tree, group)
    }

    /// Returns the total number of distinct groups in the scene.
    pub fn group_count(&self, tree: &SceneTree) -> usize {
        self.collect_all_groups(tree).len()
    }

    /// Returns the number of nodes in a specific group.
    pub fn members_in_group(&self, tree: &SceneTree, group: &str) -> usize {
        tree.get_nodes_in_group(group).len()
    }
}

impl Default for GroupDialog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdscene::node::Node;

    fn make_tree() -> SceneTree {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let player = Node::new("Player", "Node2D");
        let player_id = tree.add_child(root, player).unwrap();
        let enemy1 = Node::new("Enemy1", "Sprite2D");
        let enemy1_id = tree.add_child(root, enemy1).unwrap();
        let enemy2 = Node::new("Enemy2", "Sprite2D");
        let enemy2_id = tree.add_child(root, enemy2).unwrap();

        tree.add_to_group(player_id, "players").unwrap();
        tree.add_to_group(enemy1_id, "enemies").unwrap();
        tree.add_to_group(enemy2_id, "enemies").unwrap();
        tree.add_to_group(player_id, "saveable").unwrap();
        tree.add_to_group(enemy1_id, "saveable").unwrap();

        tree
    }

    fn node_ids(tree: &SceneTree) -> (NodeId, NodeId, NodeId, NodeId) {
        let root = tree.root_id();
        let root_node = tree.get_node(root).unwrap();
        let children: Vec<NodeId> = root_node.children().to_vec();
        (root, children[0], children[1], children[2])
    }

    #[test]
    fn new_dialog_defaults() {
        let d = GroupDialog::new();
        assert!(!d.is_visible());
        assert!(d.inspected_node().is_none());
        assert!(d.filter_text().is_empty());
    }

    #[test]
    fn open_and_close() {
        let tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();

        d.open(player_id);
        assert!(d.is_visible());
        assert_eq!(d.inspected_node(), Some(player_id));

        d.close();
        assert!(!d.is_visible());
        // inspected node persists after close
        assert_eq!(d.inspected_node(), Some(player_id));
    }

    #[test]
    fn open_clears_filter() {
        let tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.set_filter("old filter");
        d.open(player_id);
        assert!(d.filter_text().is_empty());
    }

    #[test]
    fn collect_all_groups() {
        let tree = make_tree();
        let d = GroupDialog::new();
        let groups = d.collect_all_groups(&tree);

        assert_eq!(groups.len(), 3); // enemies, players, saveable
        assert!(groups.contains_key("enemies"));
        assert!(groups.contains_key("players"));
        assert!(groups.contains_key("saveable"));
        assert_eq!(groups["enemies"].len(), 2);
        assert_eq!(groups["players"].len(), 1);
        assert_eq!(groups["saveable"].len(), 2);
    }

    #[test]
    fn filtered_groups_no_filter() {
        let tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        let entries = d.filtered_groups(&tree);
        assert_eq!(entries.len(), 3);

        // Sorted alphabetically (BTreeMap).
        assert_eq!(entries[0].name, "enemies");
        assert!(!entries[0].node_is_member); // Player is not in "enemies"
        assert_eq!(entries[1].name, "players");
        assert!(entries[1].node_is_member); // Player IS in "players"
        assert_eq!(entries[2].name, "saveable");
        assert!(entries[2].node_is_member); // Player IS in "saveable"
    }

    #[test]
    fn filtered_groups_with_filter() {
        let tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);
        d.set_filter("enem");

        let entries = d.filtered_groups(&tree);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "enemies");
    }

    #[test]
    fn filter_case_insensitive() {
        let tree = make_tree();
        let mut d = GroupDialog::new();
        d.set_filter("SAVE");

        let entries = d.filtered_groups(&tree);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "saveable");
    }

    #[test]
    fn node_groups_returns_sorted() {
        let tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        let groups = d.node_groups(&tree);
        assert_eq!(groups, vec!["players", "saveable"]);
    }

    #[test]
    fn node_groups_no_inspected_node() {
        let tree = make_tree();
        let d = GroupDialog::new();
        assert!(d.node_groups(&tree).is_empty());
    }

    #[test]
    fn add_to_group() {
        let mut tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        let result = d.add_to_group(&mut tree, "new_group").unwrap();
        assert_eq!(result.group_name, "new_group");

        let groups = d.node_groups(&tree);
        assert!(groups.contains(&"new_group".to_string()));
    }

    #[test]
    fn add_to_group_empty_name_fails() {
        let mut tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        let err = d.add_to_group(&mut tree, "").unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn add_to_group_no_inspected_node_fails() {
        let mut tree = make_tree();
        let d = GroupDialog::new();

        let err = d.add_to_group(&mut tree, "test").unwrap_err();
        assert!(err.contains("no node"));
    }

    #[test]
    fn remove_from_group() {
        let mut tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        assert!(d.node_groups(&tree).contains(&"players".to_string()));

        let result = d.remove_from_group(&mut tree, "players").unwrap();
        assert_eq!(result.group_name, "players");

        assert!(!d.node_groups(&tree).contains(&"players".to_string()));
    }

    #[test]
    fn remove_from_group_no_inspected_node_fails() {
        let mut tree = make_tree();
        let d = GroupDialog::new();

        let err = d.remove_from_group(&mut tree, "players").unwrap_err();
        assert!(err.contains("no node"));
    }

    #[test]
    fn create_group_is_add() {
        let mut tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        let result = d.create_group(&mut tree, "brand_new").unwrap();
        assert_eq!(result.group_name, "brand_new");

        assert!(d.node_groups(&tree).contains(&"brand_new".to_string()));
    }

    #[test]
    fn group_count() {
        let tree = make_tree();
        let d = GroupDialog::new();
        assert_eq!(d.group_count(&tree), 3);
    }

    #[test]
    fn members_in_group() {
        let tree = make_tree();
        let d = GroupDialog::new();
        assert_eq!(d.members_in_group(&tree, "enemies"), 2);
        assert_eq!(d.members_in_group(&tree, "players"), 1);
        assert_eq!(d.members_in_group(&tree, "nonexistent"), 0);
    }

    #[test]
    fn member_count_in_entries() {
        let tree = make_tree();
        let d = GroupDialog::new();
        let entries = d.filtered_groups(&tree);

        let enemies = entries.iter().find(|e| e.name == "enemies").unwrap();
        assert_eq!(enemies.member_count, 2);
        let saveable = entries.iter().find(|e| e.name == "saveable").unwrap();
        assert_eq!(saveable.member_count, 2);
    }

    #[test]
    fn set_inspected_node_without_open() {
        let tree = make_tree();
        let (_, player_id, enemy1_id, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        d.set_inspected_node(Some(enemy1_id));
        assert_eq!(d.inspected_node(), Some(enemy1_id));

        let groups = d.node_groups(&tree);
        assert!(groups.contains(&"enemies".to_string()));
        assert!(!groups.contains(&"players".to_string()));
    }

    #[test]
    fn add_then_remove_roundtrip() {
        let mut tree = make_tree();
        let (_, player_id, _, _) = node_ids(&tree);
        let mut d = GroupDialog::new();
        d.open(player_id);

        let before = d.group_count(&tree);
        d.add_to_group(&mut tree, "temp_group").unwrap();
        assert_eq!(d.group_count(&tree), before + 1);

        d.remove_from_group(&mut tree, "temp_group").unwrap();
        assert_eq!(d.group_count(&tree), before);
    }

    #[test]
    fn default_trait_impl() {
        let d = GroupDialog::default();
        assert!(!d.is_visible());
    }

    #[test]
    fn empty_tree_has_no_groups() {
        let tree = SceneTree::new();
        let d = GroupDialog::new();
        assert_eq!(d.group_count(&tree), 0);
        assert!(d.filtered_groups(&tree).is_empty());
    }

    #[test]
    fn filtered_groups_no_inspected_still_shows_groups() {
        let tree = make_tree();
        let d = GroupDialog::new();
        // No node inspected, but should still list all groups.
        let entries = d.filtered_groups(&tree);
        assert_eq!(entries.len(), 3);
        // All node_is_member should be false.
        assert!(entries.iter().all(|e| !e.node_is_member));
    }
}
