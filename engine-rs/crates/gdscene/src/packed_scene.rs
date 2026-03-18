//! Packed scene serialization and instancing.
//!
//! A [`PackedScene`] is a template parsed from a `.tscn` file. Calling
//! [`instance()`](PackedScene::instance) creates a fresh subtree of
//! [`Node`]s that can be inserted into a [`SceneTree`].
//!
//! The parser handles the simplified `.tscn` subset:
//! - `[gd_scene]` header
//! - `[node]` sections with `name`, `type`, and `parent` attributes
//! - Property lines (`key = value`) using the variant parser from
//!   `gdresource`.

use std::collections::HashMap;

use gdcore::error::{EngineError, EngineResult};
use gdresource::loader::parse_variant_value;
use gdvariant::Variant;

use crate::node::Node;

// ---------------------------------------------------------------------------
// NodeTemplate
// ---------------------------------------------------------------------------

/// A blueprint for a single node, extracted from a `.tscn` file.
#[derive(Debug, Clone)]
struct NodeTemplate {
    /// Node name (e.g. `"Player"`).
    name: String,
    /// Godot class name (e.g. `"Node2D"`).
    class_name: String,
    /// Parent path within the scene. `None` means this is the scene root.
    /// `"."` means direct child of root. `"Player"` means child of the node
    /// named `"Player"`, and so on for deeper paths.
    parent_path: Option<String>,
    /// Properties parsed from key=value lines.
    properties: HashMap<String, Variant>,
}

// ---------------------------------------------------------------------------
// PackedScene
// ---------------------------------------------------------------------------

/// A packed scene — a template that can be instantiated into a node subtree.
///
/// Parsed from the `.tscn` text format.
#[derive(Debug, Clone)]
pub struct PackedScene {
    /// Optional UID from the `[gd_scene]` header.
    pub uid: Option<String>,
    /// Ordered list of node templates.
    templates: Vec<NodeTemplate>,
}

impl PackedScene {
    /// Parses a `.tscn` string into a `PackedScene`.
    pub fn from_tscn(source: &str) -> EngineResult<Self> {
        let mut uid = None;
        let mut templates: Vec<NodeTemplate> = Vec::new();
        let mut current: Option<NodeTemplate> = None;

        for line in source.lines() {
            let line = line.trim();

            // Skip empty / comments.
            if line.is_empty() || line.starts_with(';') {
                continue;
            }

            // Section header.
            if line.starts_with('[') && line.ends_with(']') {
                // Flush previous node template.
                if let Some(tmpl) = current.take() {
                    templates.push(tmpl);
                }

                let inner = &line[1..line.len() - 1];

                if inner.starts_with("gd_scene") {
                    let attrs = extract_header_attrs(inner);
                    uid = attrs.get("uid").cloned();
                } else if inner.starts_with("node") {
                    let attrs = extract_header_attrs(inner);
                    let name = attrs.get("name").cloned().unwrap_or_default();
                    let class_name = attrs.get("type").cloned().unwrap_or_else(|| "Node".into());
                    let parent_path = attrs.get("parent").cloned();

                    current = Some(NodeTemplate {
                        name,
                        class_name,
                        parent_path,
                        properties: HashMap::new(),
                    });
                }
                // Ignore other sections (ext_resource, sub_resource, etc.)
                // for this simplified parser.
                continue;
            }

            // Property line: key = value
            if let Some(ref mut tmpl) = current {
                if let Some((key, value_str)) = line.split_once('=') {
                    let key = key.trim();
                    let value_str = value_str.trim();
                    match parse_variant_value(value_str) {
                        Ok(value) => {
                            tmpl.properties.insert(key.to_string(), value);
                        }
                        Err(_) => {
                            // Skip values we cannot parse rather than fail.
                            tracing::warn!("skipping unparseable value for key '{key}': {value_str}");
                        }
                    }
                }
            }
        }

        // Flush last template.
        if let Some(tmpl) = current.take() {
            templates.push(tmpl);
        }

        if templates.is_empty() {
            return Err(EngineError::Parse(
                "no [node] sections found in .tscn".into(),
            ));
        }

        Ok(Self { uid, templates })
    }

    /// Instantiates the packed scene, returning the root node and a flat
    /// list of all nodes in the subtree.
    ///
    /// The returned nodes are not yet attached to any [`SceneTree`]. The
    /// caller should add the root to the tree and then add each subsequent
    /// node as a child of the appropriate parent.
    ///
    /// Returns `(nodes, root_index)` where `root_index` is always 0.
    pub fn instance(&self) -> EngineResult<Vec<Node>> {
        if self.templates.is_empty() {
            return Err(EngineError::InvalidOperation(
                "packed scene has no nodes".into(),
            ));
        }

        // First template must be the scene root (no parent_path).
        let root_tmpl = &self.templates[0];
        if root_tmpl.parent_path.is_some() {
            return Err(EngineError::Parse(
                "first [node] section must be the scene root (no parent attribute)".into(),
            ));
        }

        let mut nodes: Vec<Node> = Vec::new();
        // Map from scene-local path -> index in `nodes`.
        let mut path_to_index: HashMap<String, usize> = HashMap::new();

        // Create root node.
        let mut root_node = Node::new(&root_tmpl.name, &root_tmpl.class_name);
        for (key, value) in &root_tmpl.properties {
            root_node.set_property(key, value.clone());
        }
        path_to_index.insert(".".into(), 0);
        // Also map by name for child lookup.
        path_to_index.insert(root_tmpl.name.clone(), 0);
        nodes.push(root_node);

        // Process remaining nodes.
        for tmpl in &self.templates[1..] {
            let parent_path = tmpl.parent_path.as_deref().unwrap_or(".");
            let parent_idx = path_to_index.get(parent_path).copied().ok_or_else(|| {
                EngineError::Parse(format!(
                    "parent path '{parent_path}' not found for node '{}'",
                    tmpl.name
                ))
            })?;

            let mut node = Node::new(&tmpl.name, &tmpl.class_name);
            for (key, value) in &tmpl.properties {
                node.set_property(key, value.clone());
            }

            let child_id = node.id();
            let child_idx = nodes.len();

            // Compute this node's scene-local path for future children.
            let node_path = if parent_path == "." {
                tmpl.name.clone()
            } else {
                format!("{}/{}", parent_path, tmpl.name)
            };
            path_to_index.insert(node_path, child_idx);

            // Wire parent/child relationship.
            node.set_parent(Some(nodes[parent_idx].id()));
            nodes[parent_idx].add_child_id(child_id);

            nodes.push(node);
        }

        Ok(nodes)
    }

    /// Returns the number of node templates in this packed scene.
    pub fn node_count(&self) -> usize {
        self.templates.len()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts `key="value"` pairs from a section header string.
///
/// Replicates the logic from `gdresource::loader` for `.tscn` headers.
fn extract_header_attrs(header: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let mut remaining = header;

    // Skip the section keyword (first word).
    if let Some(idx) = remaining.find(' ') {
        remaining = &remaining[idx..];
    } else {
        return attrs;
    }

    while let Some(eq_idx) = remaining.find('=') {
        let key = remaining[..eq_idx].trim();
        remaining = &remaining[eq_idx + 1..];

        if remaining.starts_with('"') {
            remaining = &remaining[1..];
            if let Some(end_quote) = remaining.find('"') {
                let value = &remaining[..end_quote];
                attrs.insert(key.to_string(), value.to_string());
                remaining = &remaining[end_quote + 1..];
            } else {
                break;
            }
        } else {
            let end = remaining.find(' ').unwrap_or(remaining.len());
            let value = &remaining[..end];
            attrs.insert(key.to_string(), value.to_string());
            remaining = &remaining[end..];
        }
    }

    attrs
}

// ---------------------------------------------------------------------------
// Utility: add instanced nodes to a SceneTree
// ---------------------------------------------------------------------------

/// Adds all nodes from a [`PackedScene::instance()`] call into a
/// [`SceneTree`] under the given parent.
///
/// Returns the [`NodeId`] of the instanced scene's root node.
pub fn add_packed_scene_to_tree(
    tree: &mut crate::scene_tree::SceneTree,
    parent_id: crate::node::NodeId,
    scene: &PackedScene,
) -> EngineResult<crate::node::NodeId> {
    let nodes = scene.instance()?;
    if nodes.is_empty() {
        return Err(EngineError::InvalidOperation(
            "instanced scene produced no nodes".into(),
        ));
    }

    // We need to map from old IDs (generated during instance()) to new IDs
    // (assigned when we add to the tree). But since we already created the
    // nodes with their IDs, we can add them directly.
    //
    // The first node is the scene root — add it under the provided parent.
    // Subsequent nodes already have their parent set from instance().
    // We need to re-add them properly through the tree API.

    // Strategy: collect all the nodes, then add them in order.
    // The instance() output already has parent/child wired by NodeId,
    // but those nodes aren't in the tree yet. We'll re-create them.

    let mut old_to_new: HashMap<crate::node::NodeId, crate::node::NodeId> = HashMap::new();

    // Add root of the instanced scene.
    let scene_root = &nodes[0];
    let mut new_root = Node::new(scene_root.name(), scene_root.class_name());
    for (key, value) in scene_root.properties() {
        new_root.set_property(key, value.clone());
    }
    for group in scene_root.groups() {
        new_root.add_to_group(group.clone());
    }
    let new_root_id = new_root.id();
    old_to_new.insert(scene_root.id(), new_root_id);
    tree.add_child(parent_id, new_root)?;

    // Add remaining nodes.
    for node in &nodes[1..] {
        let old_parent_id = node.parent().ok_or_else(|| {
            EngineError::InvalidOperation(format!(
                "instanced node '{}' has no parent",
                node.name()
            ))
        })?;
        let &new_parent_id = old_to_new.get(&old_parent_id).ok_or_else(|| {
            EngineError::InvalidOperation(format!(
                "parent for '{}' not yet added to tree",
                node.name()
            ))
        })?;

        let mut new_node = Node::new(node.name(), node.class_name());
        for (key, value) in node.properties() {
            new_node.set_property(key, value.clone());
        }
        for group in node.groups() {
            new_node.add_to_group(group.clone());
        }
        let new_node_id = new_node.id();
        old_to_new.insert(node.id(), new_node_id);
        tree.add_child(new_parent_id, new_node)?;
    }

    Ok(new_root_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_tree::SceneTree;
    use gdcore::math::Vector2;

    const SIMPLE_TSCN: &str = r#"
[gd_scene format=3 uid="uid://abc123"]

[node name="Root" type="Node"]

[node name="Player" type="Node2D" parent="."]
position = Vector2(100, 200)

[node name="Sprite" type="Sprite2D" parent="Player"]
"#;

    #[test]
    fn parse_simple_tscn() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        assert_eq!(scene.node_count(), 3);
        assert_eq!(scene.uid.as_deref(), Some("uid://abc123"));
    }

    #[test]
    fn instance_creates_hierarchy() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(nodes.len(), 3);

        // Root
        assert_eq!(nodes[0].name(), "Root");
        assert_eq!(nodes[0].class_name(), "Node");
        assert!(nodes[0].parent().is_none());
        assert_eq!(nodes[0].children().len(), 1);

        // Player
        assert_eq!(nodes[1].name(), "Player");
        assert_eq!(nodes[1].class_name(), "Node2D");
        assert_eq!(nodes[1].parent(), Some(nodes[0].id()));
        assert_eq!(nodes[1].children().len(), 1);

        // Player has a position property.
        assert_eq!(
            nodes[1].get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );

        // Sprite
        assert_eq!(nodes[2].name(), "Sprite");
        assert_eq!(nodes[2].class_name(), "Sprite2D");
        assert_eq!(nodes[2].parent(), Some(nodes[1].id()));
        assert!(nodes[2].children().is_empty());
    }

    #[test]
    fn add_to_tree_and_verify() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        // Tree should now have: root -> Root -> Player -> Sprite (4 nodes total).
        assert_eq!(tree.node_count(), 4);

        // Verify paths.
        assert_eq!(
            tree.node_path(scene_root_id).unwrap(),
            "/root/Root"
        );

        let player_id = tree.get_node_by_path("/root/Root/Player").unwrap();
        let player = tree.get_node(player_id).unwrap();
        assert_eq!(player.class_name(), "Node2D");
        assert_eq!(
            player.get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );

        let sprite_id = tree.get_node_by_path("/root/Root/Player/Sprite").unwrap();
        let sprite = tree.get_node(sprite_id).unwrap();
        assert_eq!(sprite.class_name(), "Sprite2D");
    }

    #[test]
    fn parse_tscn_no_nodes_fails() {
        let bad = "[gd_scene format=3]\n";
        let result = PackedScene::from_tscn(bad);
        assert!(result.is_err());
    }

    #[test]
    fn deeper_nesting() {
        let tscn = r#"
[gd_scene format=3]

[node name="World" type="Node"]

[node name="Level" type="Node" parent="."]

[node name="Enemies" type="Node" parent="Level"]

[node name="Boss" type="Node2D" parent="Level/Enemies"]
health = 100
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes.len(), 4);

        assert_eq!(nodes[3].name(), "Boss");
        assert_eq!(nodes[3].get_property("health"), Variant::Int(100));
        assert_eq!(nodes[3].parent(), Some(nodes[2].id()));
    }
}
