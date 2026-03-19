//! Packed scene serialization and instancing.
//!
//! A [`PackedScene`] is a template parsed from a `.tscn` file. Calling
//! [`instance()`](PackedScene::instance) creates a fresh subtree of
//! [`Node`]s that can be inserted into a [`crate::SceneTree`].
//!
//! The parser handles the simplified `.tscn` subset:
//! - `[gd_scene]` header
//! - `[node]` sections with `name`, `type`, and `parent` attributes
//! - Property lines (`key = value`) using the variant parser from
//!   `gdresource`.

use std::collections::HashMap;

use gdcore::error::{EngineError, EngineResult};
use gdobject::signal::Connection;
use gdresource::loader::parse_variant_value;
use gdvariant::Variant;

use crate::node::{Node, NodeId};

// ---------------------------------------------------------------------------
// SceneConnection
// ---------------------------------------------------------------------------

/// A signal connection parsed from a `[connection]` section in a `.tscn` file.
///
/// ```text
/// [connection signal="pressed" from="Button" to="." method="_on_button_pressed" flags=3]
/// ```
#[derive(Debug, Clone)]
pub struct SceneConnection {
    /// The signal name (e.g. `"pressed"`).
    pub signal_name: String,
    /// Relative path from the scene root to the emitting node.
    pub from_path: String,
    /// Relative path from the scene root to the receiving node.
    /// `"."` means the scene root itself.
    pub to_path: String,
    /// The method name to call on the target node.
    pub method_name: String,
    /// Optional connection flags (defaults to 0).
    pub flags: u32,
}

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
    /// Groups this node belongs to, parsed from `groups=["a", "b"]`.
    groups: Vec<String>,
    /// Whether this node has a scene-unique name (`%` prefix).
    unique_name: bool,
    /// Instance resource reference (e.g. `ExtResource("1_abc")`), if this
    /// node instances a sub-scene.
    instance: Option<String>,
    /// Script resource path resolved from `script = ExtResource("id")`.
    script_path: Option<String>,
}

/// An external resource reference parsed from `[ext_resource]` sections.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ExtResourceEntry {
    /// The resource type (e.g. `"Script"`, `"PackedScene"`, `"Texture2D"`).
    res_type: String,
    /// The resource path (e.g. `"res://scripts/player.gd"`).
    path: String,
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
    /// Signal connections parsed from `[connection]` sections.
    connections: Vec<SceneConnection>,
    /// External resources: id -> entry.
    #[allow(dead_code)]
    ext_resources: HashMap<String, ExtResourceEntry>,
}

impl PackedScene {
    /// Parses a `.tscn` string into a `PackedScene`.
    pub fn from_tscn(source: &str) -> EngineResult<Self> {
        let mut uid = None;
        let mut templates: Vec<NodeTemplate> = Vec::new();
        let mut connections: Vec<SceneConnection> = Vec::new();
        let mut ext_resources: HashMap<String, ExtResourceEntry> = HashMap::new();
        let mut current: Option<NodeTemplate> = None;

        for (line_num, line) in source.lines().enumerate() {
            let _line_num = line_num + 1; // 1-based, for error reporting
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
                } else if inner.starts_with("connection") {
                    let attrs = extract_header_attrs(inner);
                    // All four required attributes must be present.
                    let signal_name = attrs.get("signal").cloned();
                    let from_path = attrs.get("from").cloned();
                    let to_path = attrs.get("to").cloned();
                    let method_name = attrs.get("method").cloned();

                    match (signal_name, from_path, to_path, method_name) {
                        (Some(signal), Some(from), Some(to), Some(method)) => {
                            let flags = attrs
                                .get("flags")
                                .and_then(|v| v.parse::<u32>().ok())
                                .unwrap_or(0);
                            connections.push(SceneConnection {
                                signal_name: signal,
                                from_path: from,
                                to_path: to,
                                method_name: method,
                                flags,
                            });
                        }
                        _ => {
                            tracing::warn!(
                                "skipping malformed [connection] — missing required attributes"
                            );
                        }
                    }
                } else if inner.starts_with("node") {
                    let attrs = extract_header_attrs(inner);
                    let raw_name = attrs.get("name").cloned().unwrap_or_default();

                    // Scene unique name: `%` prefix in Godot editor.
                    let (unique_name, name) = if let Some(stripped) = raw_name.strip_prefix('%') {
                        (true, stripped.to_string())
                    } else {
                        (false, raw_name)
                    };

                    let class_name = attrs.get("type").cloned().unwrap_or_else(|| "Node".into());
                    let parent_path = attrs.get("parent").cloned();

                    // Parse groups=["group1", "group2"] attribute.
                    let groups = attrs
                        .get("groups")
                        .map(|g| parse_groups_attr(g))
                        .unwrap_or_default();

                    // Parse instance=ExtResource("id") attribute.
                    let instance = attrs.get("instance").cloned();

                    current = Some(NodeTemplate {
                        name,
                        class_name,
                        parent_path,
                        properties: HashMap::new(),
                        groups,
                        unique_name,
                        instance,
                        script_path: None,
                    });
                } else if inner.starts_with("ext_resource") {
                    let attrs = extract_header_attrs(inner);
                    if let (Some(id), Some(path)) =
                        (attrs.get("id").cloned(), attrs.get("path").cloned())
                    {
                        let res_type = attrs.get("type").cloned().unwrap_or_default();
                        ext_resources.insert(id, ExtResourceEntry { res_type, path });
                    }
                }
                // Ignore other sections (sub_resource, etc.)
                // for this simplified parser.
                continue;
            }

            // Property line: key = value
            if let Some(ref mut tmpl) = current {
                if let Some((key, value_str)) = line.split_once('=') {
                    let key = key.trim();
                    let value_str = value_str.trim();

                    // Handle ExtResource references (e.g. `script = ExtResource("1_abc")`).
                    if let Some(ext_id) = parse_ext_resource_ref(value_str) {
                        if key == "script" {
                            if let Some(entry) = ext_resources.get(&ext_id) {
                                tmpl.script_path = Some(entry.path.clone());
                            }
                        }
                        // Store the raw reference as a string property too.
                        tmpl.properties
                            .insert(key.to_string(), Variant::String(value_str.to_string()));
                        continue;
                    }

                    match parse_variant_value(value_str) {
                        Ok(value) => {
                            tmpl.properties.insert(key.to_string(), value);
                        }
                        Err(_) => {
                            // Skip values we cannot parse rather than fail.
                            tracing::warn!(
                                "skipping unparseable value for key '{key}': {value_str}"
                            );
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

        Ok(Self {
            uid,
            templates,
            connections,
            ext_resources,
        })
    }

    /// Instantiates the packed scene, returning the root node and a flat
    /// list of all nodes in the subtree.
    ///
    /// The returned nodes are not yet attached to any [`crate::SceneTree`]. The
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
        for group in &root_tmpl.groups {
            root_node.add_to_group(group.clone());
        }
        root_node.set_unique_name(root_tmpl.unique_name);
        if let Some(ref inst) = root_tmpl.instance {
            root_node.set_property("_instance", Variant::String(inst.clone()));
        }
        if let Some(ref script_path) = root_tmpl.script_path {
            root_node.set_property("_script_path", Variant::String(script_path.clone()));
        }
        // Root node owns itself (owner = None signals it IS the owner).
        path_to_index.insert(".".into(), 0);
        // Also map by name for child lookup.
        path_to_index.insert(root_tmpl.name.clone(), 0);
        let root_id = root_node.id();
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
            for group in &tmpl.groups {
                node.add_to_group(group.clone());
            }
            node.set_unique_name(tmpl.unique_name);
            if let Some(ref inst) = tmpl.instance {
                node.set_property("_instance", Variant::String(inst.clone()));
            }
            if let Some(ref script_path) = tmpl.script_path {
                node.set_property("_script_path", Variant::String(script_path.clone()));
            }
            // Owner is the scene root for all non-root nodes.
            node.set_owner(Some(root_id));

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

    /// Returns the parsed signal connections.
    pub fn connections(&self) -> &[SceneConnection] {
        &self.connections
    }

    /// Returns the number of signal connections in this packed scene.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts `key="value"` pairs from a section header string.
///
/// Replicates the logic from `gdresource::loader` for `.tscn` headers.
/// Also handles bracket-delimited values like `groups=["a", "b"]` and
/// function-call values like `instance=ExtResource("1_abc")`.
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
        } else if remaining.starts_with('[') {
            // Bracket-delimited value like ["group1", "group2"].
            if let Some(end_bracket) = remaining.find(']') {
                let value = &remaining[..=end_bracket];
                attrs.insert(key.to_string(), value.to_string());
                remaining = &remaining[end_bracket + 1..];
            } else {
                break;
            }
        } else {
            // Unquoted value — may contain parentheses like ExtResource("id").
            // Find the end by tracking paren depth.
            let mut end = 0;
            let mut paren_depth = 0i32;
            for (i, ch) in remaining.char_indices() {
                match ch {
                    '(' => paren_depth += 1,
                    ')' => {
                        paren_depth -= 1;
                        if paren_depth == 0 {
                            end = i + 1;
                            break;
                        }
                    }
                    ' ' if paren_depth == 0 => {
                        end = i;
                        break;
                    }
                    _ => {}
                }
            }
            if end == 0 {
                end = remaining.len();
            }
            let value = &remaining[..end];
            attrs.insert(key.to_string(), value.to_string());
            remaining = &remaining[end..];
        }
    }

    attrs
}

/// Parses a groups attribute value like `["enemy", "damageable"]` into a
/// `Vec<String>`.
fn parse_groups_attr(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    // Strip outer brackets.
    let inner = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    inner
        .split(',')
        .filter_map(|s| {
            let s = s.trim().trim_matches('"');
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect()
}

/// Parses an `ExtResource("id")` value string and returns the id.
fn parse_ext_resource_ref(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let inner = trimmed.strip_prefix("ExtResource(")?;
    let inner = inner.strip_suffix(')')?;
    let id = inner.trim().trim_matches('"');
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

// ---------------------------------------------------------------------------
// Utility: add instanced nodes to a SceneTree
// ---------------------------------------------------------------------------

/// Adds all nodes from a [`PackedScene::instance()`] call into a
/// [`crate::SceneTree`] under the given parent.
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
    new_root.set_unique_name(scene_root.is_unique_name());
    let new_root_id = new_root.id();
    old_to_new.insert(scene_root.id(), new_root_id);
    tree.add_child(parent_id, new_root)?;

    // Add remaining nodes.
    for node in &nodes[1..] {
        let old_parent_id = node.parent().ok_or_else(|| {
            EngineError::InvalidOperation(format!("instanced node '{}' has no parent", node.name()))
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
        new_node.set_unique_name(node.is_unique_name());
        // Set owner to the new scene root.
        new_node.set_owner(Some(new_root_id));
        let new_node_id = new_node.id();
        old_to_new.insert(node.id(), new_node_id);
        tree.add_child(new_parent_id, new_node)?;
    }

    // Wire any signal connections from the packed scene.
    wire_connections(tree, new_root_id, &scene.connections);

    Ok(new_root_id)
}

/// Wires signal connections from a packed scene into the scene tree.
///
/// After a packed scene has been instanced and added to the tree via
/// [`add_packed_scene_to_tree`], call this function to resolve the
/// `from`/`to` paths in each [`SceneConnection`] to [`NodeId`]s and
/// register the connections in the tree's signal stores.
///
/// - `"."` in `from_path` or `to_path` refers to the scene root.
/// - Non-existent paths produce a warning but do not cause an error.
pub fn wire_connections(
    tree: &mut crate::scene_tree::SceneTree,
    root_id: NodeId,
    connections: &[SceneConnection],
) {
    for conn in connections {
        // Resolve "from" path relative to scene root.
        let from_id = if conn.from_path == "." {
            Some(root_id)
        } else {
            tree.get_node_relative(root_id, &conn.from_path)
        };

        // Resolve "to" path relative to scene root.
        let to_id = if conn.to_path == "." {
            Some(root_id)
        } else {
            tree.get_node_relative(root_id, &conn.to_path)
        };

        match (from_id, to_id) {
            (Some(from), Some(to)) => {
                // Look up the target node's ObjectId for the Connection.
                let to_object_id = to.object_id();
                let connection = Connection::new(to_object_id, &conn.method_name);
                tree.connect_signal(from, &conn.signal_name, connection);
            }
            (None, _) => {
                tracing::warn!(
                    "wire_connections: from path '{}' not found for signal '{}'",
                    conn.from_path,
                    conn.signal_name,
                );
            }
            (_, None) => {
                tracing::warn!(
                    "wire_connections: to path '{}' not found for signal '{}'",
                    conn.to_path,
                    conn.signal_name,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_tree::SceneTree;
    use gdcore::math::Vector2;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const SIMPLE_TSCN: &str = r#"
[gd_scene format=3 uid="uid://abc123"]

[node name="Root" type="Node"]

[node name="Player" type="Node2D" parent="."]
position = Vector2(100, 200)

[node name="Sprite" type="Sprite2D" parent="Player"]
"#;

    const SIGNALS_TSCN: &str = r#"
[gd_scene format=3 uid="uid://signals_test"]

[node name="Root" type="Control"]

[node name="Button" type="Button" parent="."]

[node name="Player" type="Node2D" parent="."]

[node name="Area2D" type="Area2D" parent="Player"]

[connection signal="pressed" from="Button" to="." method="_on_button_pressed"]
[connection signal="body_entered" from="Player/Area2D" to="Player" method="_on_body_entered" flags=3]
[connection signal="mouse_entered" from="Button" to="Player" method="_on_mouse_entered"]
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
        assert_eq!(tree.node_path(scene_root_id).unwrap(), "/root/Root");

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

    #[test]
    fn parse_empty_scene_file_fails() {
        let result = PackedScene::from_tscn("");
        assert!(result.is_err());
    }

    #[test]
    fn parse_scene_with_only_gd_scene_header_fails() {
        let tscn = "[gd_scene format=3]\n";
        let result = PackedScene::from_tscn(tscn);
        assert!(result.is_err());
    }

    #[test]
    fn parse_scene_with_unknown_attributes_still_works() {
        let tscn = r#"
[gd_scene format=3 unknown_attr="value"]

[node name="Root" type="Node" some_extra="ignored"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        assert_eq!(scene.node_count(), 1);
    }

    #[test]
    fn malformed_tscn_node_without_root() {
        let tscn = r#"
[gd_scene format=3]

[node name="Child" type="Node" parent="."]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let result = scene.instance();
        assert!(result.is_err());
    }

    #[test]
    fn malformed_tscn_child_references_missing_parent() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Child" type="Node" parent="NonExistent"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let result = scene.instance();
        assert!(result.is_err());
    }

    #[test]
    fn parse_scene_with_comments() {
        let tscn = r#"
; This is a comment
[gd_scene format=3]
; Another comment

[node name="Root" type="Node"]
; Property comment
value = 42
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[0].get_property("value"), Variant::Int(42));
    }

    #[test]
    fn instance_node_without_type_defaults_to_node() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[0].class_name(), "Node");
    }

    // -----------------------------------------------------------------------
    // Signal connection parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_single_connection() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_pressed"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        assert_eq!(scene.connection_count(), 1);

        let conn = &scene.connections()[0];
        assert_eq!(conn.signal_name, "pressed");
        assert_eq!(conn.from_path, "Button");
        assert_eq!(conn.to_path, ".");
        assert_eq!(conn.method_name, "_on_pressed");
        assert_eq!(conn.flags, 0);
    }

    #[test]
    fn parse_multiple_connections() {
        let scene = PackedScene::from_tscn(SIGNALS_TSCN).unwrap();
        assert_eq!(scene.connection_count(), 3);

        assert_eq!(scene.connections()[0].signal_name, "pressed");
        assert_eq!(scene.connections()[1].signal_name, "body_entered");
        assert_eq!(scene.connections()[2].signal_name, "mouse_entered");
    }

    #[test]
    fn parse_connection_with_flags() {
        let scene = PackedScene::from_tscn(SIGNALS_TSCN).unwrap();
        let body_conn = &scene.connections()[1];
        assert_eq!(body_conn.signal_name, "body_entered");
        assert_eq!(body_conn.from_path, "Player/Area2D");
        assert_eq!(body_conn.to_path, "Player");
        assert_eq!(body_conn.method_name, "_on_body_entered");
        assert_eq!(body_conn.flags, 3);
    }

    #[test]
    fn scene_with_no_connections_still_works() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        assert_eq!(scene.connection_count(), 0);
        assert!(scene.connections().is_empty());

        // Instancing and adding to tree should still work fine.
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        assert_eq!(tree.node_count(), 4);
        // No signal stores should be created.
        assert!(tree.signal_store(scene_root).is_none());
    }

    #[test]
    fn malformed_connection_missing_attributes() {
        // Missing "method" attribute — should be skipped gracefully.
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="."]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        // Malformed connection is silently skipped.
        assert_eq!(scene.connection_count(), 0);
        assert_eq!(scene.node_count(), 2);
    }

    #[test]
    fn instance_scene_and_verify_connections_exist() {
        let scene = PackedScene::from_tscn(SIGNALS_TSCN).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        // Button node should have signal "pressed" connected.
        let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();
        let store = tree
            .signal_store(button_id)
            .expect("Button should have a signal store");
        let pressed = store
            .get_signal("pressed")
            .expect("should have 'pressed' signal");
        assert_eq!(pressed.connection_count(), 1);
        // The target should be the scene root (to=".").
        assert_eq!(pressed.connections()[0].method, "_on_button_pressed");
        assert_eq!(pressed.connections()[0].target_id, scene_root.object_id());

        // Button also has mouse_entered connected.
        let mouse = store
            .get_signal("mouse_entered")
            .expect("should have 'mouse_entered'");
        assert_eq!(mouse.connection_count(), 1);
        assert_eq!(mouse.connections()[0].method, "_on_mouse_entered");

        // Player/Area2D should have body_entered connected.
        let area_id = tree.get_node_by_path("/root/Root/Player/Area2D").unwrap();
        let area_store = tree
            .signal_store(area_id)
            .expect("Area2D should have a signal store");
        let body = area_store
            .get_signal("body_entered")
            .expect("should have 'body_entered'");
        assert_eq!(body.connection_count(), 1);
        assert_eq!(body.connections()[0].method, "_on_body_entered");
    }

    #[test]
    fn connection_from_child_to_root() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Child" type="Node" parent="."]

[connection signal="done" from="Child" to="." method="_on_child_done"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let child_id = tree.get_node_by_path("/root/Root/Child").unwrap();
        let store = tree
            .signal_store(child_id)
            .expect("Child should have signal store");
        let sig = store.get_signal("done").unwrap();
        assert_eq!(sig.connection_count(), 1);
        assert_eq!(sig.connections()[0].target_id, scene_root.object_id());
        assert_eq!(sig.connections()[0].method, "_on_child_done");
    }

    #[test]
    fn connection_from_nested_child_to_parent() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Parent" type="Node" parent="."]

[node name="Deep" type="Node" parent="Parent"]

[connection signal="alert" from="Parent/Deep" to="Parent" method="_on_deep_alert"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let deep_id = tree.get_node_by_path("/root/Root/Parent/Deep").unwrap();
        let parent_id = tree.get_node_by_path("/root/Root/Parent").unwrap();

        let store = tree
            .signal_store(deep_id)
            .expect("Deep should have signal store");
        let sig = store.get_signal("alert").unwrap();
        assert_eq!(sig.connection_count(), 1);
        assert_eq!(sig.connections()[0].target_id, parent_id.object_id());
        assert_eq!(sig.connections()[0].method, "_on_deep_alert");
    }

    #[test]
    fn connection_nonexistent_from_path_warns_no_crash() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[connection signal="pressed" from="NonExistent" to="." method="_on_pressed"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        assert_eq!(scene.connection_count(), 1);

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        // Should not crash — just logs a warning.
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        // No signal store should have been created since from_path didn't resolve.
        assert!(tree.signal_store(scene_root).is_none());
    }

    #[test]
    fn connection_nonexistent_to_path_warns_no_crash() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="Ghost" method="_on_pressed"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        assert_eq!(scene.connection_count(), 1);

        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let _scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();
        // No signal store since connection couldn't be wired.
        assert!(tree.signal_store(button_id).is_none());
    }

    #[test]
    fn emit_signal_after_instancing() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_pressed"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let button_id = tree.get_node_by_path("/root/Root/Button").unwrap();

        // The connection was wired without a callback. Attach a callback now
        // to test emission works end-to-end.
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = call_count.clone();
        let target_object_id = scene_root.object_id();
        tree.connect_signal(
            button_id,
            "pressed",
            Connection::with_callback(target_object_id, "_on_pressed_cb", move |_args| {
                counter.fetch_add(1, Ordering::SeqCst);
                Variant::Nil
            }),
        );

        // Emit the signal and verify the callback fires.
        let results = tree.emit_signal(button_id, "pressed", &[]);
        // Two connections: the original from tscn (no callback -> Nil) + our callback.
        assert_eq!(results.len(), 2);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // Emit again.
        tree.emit_signal(button_id, "pressed", &[]);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn parse_fixture_file() {
        let fixture = include_str!("../fixtures/scenes/with_signals.tscn");
        let scene = PackedScene::from_tscn(fixture).unwrap();
        assert_eq!(scene.node_count(), 4);
        assert_eq!(scene.connection_count(), 3);
        assert_eq!(scene.uid.as_deref(), Some("uid://signals_test"));
    }

    // -----------------------------------------------------------------------
    // Groups from .tscn
    // -----------------------------------------------------------------------

    #[test]
    fn parse_groups_from_tscn() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Enemy" type="Node2D" parent="." groups=["enemies", "damageable"]]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(nodes[1].is_in_group("enemies"));
        assert!(nodes[1].is_in_group("damageable"));
        assert!(!nodes[1].is_in_group("players"));
    }

    #[test]
    fn groups_survive_add_to_tree() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Player" type="Node2D" parent="." groups=["players", "controllable"]]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let player_id = tree.get_node_by_path("/root/Root/Player").unwrap();
        let player = tree.get_node(player_id).unwrap();
        assert!(player.is_in_group("players"));
        assert!(player.is_in_group("controllable"));
    }

    #[test]
    fn parse_single_group() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node" groups=["persistent"]]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert!(nodes[0].is_in_group("persistent"));
    }

    // -----------------------------------------------------------------------
    // Scene unique names (% prefix)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_unique_name_prefix() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="%HealthBar" type="Control" parent="."]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[1].name(), "HealthBar");
        assert!(nodes[1].is_unique_name());
    }

    #[test]
    fn non_unique_name_flag_is_false() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();
        for node in &nodes {
            assert!(!node.is_unique_name());
        }
    }

    // -----------------------------------------------------------------------
    // Property overrides (instance attribute)
    // -----------------------------------------------------------------------

    #[test]
    fn parse_instance_attribute() {
        let tscn = r#"
[gd_scene format=3]

[ext_resource type="PackedScene" uid="uid://abc" path="res://enemy.tscn" id="1_abc"]

[node name="Root" type="Node"]

[node name="Enemy1" parent="." instance=ExtResource("1_abc")]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[1].name(), "Enemy1");
        assert_eq!(
            nodes[1].get_property("_instance"),
            Variant::String("ExtResource(\"1_abc\")".into())
        );
    }

    #[test]
    fn node_without_instance_has_no_instance_property() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[1].get_property("_instance"), Variant::Nil);
    }

    // -----------------------------------------------------------------------
    // Owner tracking
    // -----------------------------------------------------------------------

    #[test]
    fn root_node_has_no_owner() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();
        assert!(nodes[0].owner().is_none());
    }

    #[test]
    fn child_nodes_owner_is_scene_root() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();
        let root_id = nodes[0].id();
        for node in &nodes[1..] {
            assert_eq!(node.owner(), Some(root_id));
        }
    }

    #[test]
    fn owner_set_in_tree_after_instancing() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let player_id = tree.get_node_by_path("/root/Root/Player").unwrap();
        let player = tree.get_node(player_id).unwrap();
        assert_eq!(player.owner(), Some(scene_root_id));

        let sprite_id = tree.get_node_by_path("/root/Root/Player/Sprite").unwrap();
        let sprite = tree.get_node(sprite_id).unwrap();
        assert_eq!(sprite.owner(), Some(scene_root_id));
    }

    // -----------------------------------------------------------------------
    // get_node_or_null
    // -----------------------------------------------------------------------

    #[test]
    fn get_node_or_null_relative() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(a_id, b).unwrap();

        assert_eq!(tree.get_node_or_null(root, "A/B"), Some(b_id));
        assert_eq!(tree.get_node_or_null(root, "A/Missing"), None);
    }

    #[test]
    fn get_node_or_null_absolute() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        assert_eq!(tree.get_node_or_null(a_id, "/root/A"), Some(a_id));
        assert_eq!(tree.get_node_or_null(a_id, "/root/Nope"), None);
    }

    // -----------------------------------------------------------------------
    // get_index
    // -----------------------------------------------------------------------

    #[test]
    fn get_index_returns_correct_position() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("A", "Node");
        let a_id = tree.add_child(root, a).unwrap();
        let b = Node::new("B", "Node");
        let b_id = tree.add_child(root, b).unwrap();
        let c = Node::new("C", "Node");
        let c_id = tree.add_child(root, c).unwrap();

        assert_eq!(tree.get_index(a_id), Some(0));
        assert_eq!(tree.get_index(b_id), Some(1));
        assert_eq!(tree.get_index(c_id), Some(2));
    }

    #[test]
    fn get_index_root_has_no_index() {
        let tree = SceneTree::new();
        assert_eq!(tree.get_index(tree.root_id()), None);
    }

    // -----------------------------------------------------------------------
    // duplicate_subtree
    // -----------------------------------------------------------------------

    #[test]
    fn duplicate_subtree_deep_clone() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let a = Node::new("A", "Node2D");
        let a_id = tree.add_child(root, a).unwrap();

        let b = Node::new("B", "Sprite2D");
        let b_id = tree.add_child(a_id, b).unwrap();

        // Set a property on B.
        tree.get_node_mut(b_id)
            .unwrap()
            .set_property("frame", Variant::Int(5));
        tree.add_to_group(b_id, "sprites").unwrap();

        let cloned = tree.duplicate_subtree(a_id).unwrap();
        assert_eq!(cloned.len(), 2);

        // New IDs.
        assert_ne!(cloned[0].id(), a_id);
        assert_ne!(cloned[1].id(), b_id);

        // Names and classes preserved.
        assert_eq!(cloned[0].name(), "A");
        assert_eq!(cloned[0].class_name(), "Node2D");
        assert_eq!(cloned[1].name(), "B");
        assert_eq!(cloned[1].class_name(), "Sprite2D");

        // Properties cloned.
        assert_eq!(cloned[1].get_property("frame"), Variant::Int(5));

        // Groups cloned.
        assert!(cloned[1].is_in_group("sprites"));

        // Parent/child wiring uses new IDs.
        assert!(cloned[0].parent().is_none()); // root of clone has no parent
        assert_eq!(cloned[1].parent(), Some(cloned[0].id()));
        assert_eq!(cloned[0].children(), &[cloned[1].id()]);
    }

    #[test]
    fn duplicate_subtree_nonexistent_fails() {
        let tree = SceneTree::new();
        let fake = crate::node::NodeId::next();
        assert!(tree.duplicate_subtree(fake).is_err());
    }

    #[test]
    fn duplicate_single_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let a = Node::new("Leaf", "Node");
        let a_id = tree.add_child(root, a).unwrap();

        let cloned = tree.duplicate_subtree(a_id).unwrap();
        assert_eq!(cloned.len(), 1);
        assert_eq!(cloned[0].name(), "Leaf");
        assert_ne!(cloned[0].id(), a_id);
        assert!(cloned[0].children().is_empty());
    }

    // -----------------------------------------------------------------------
    // Real Godot 4.6.1 export format fixtures
    // -----------------------------------------------------------------------

    const COMPLEX_2D: &str = include_str!("../fixtures/real_godot/complex_2d.tscn");
    const UI_WITH_THEME: &str = include_str!("../fixtures/real_godot/ui_with_theme.tscn");

    #[test]
    fn complex_2d_parses_successfully() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        assert_eq!(scene.node_count(), 6);
        assert_eq!(scene.uid.as_deref(), Some("uid://abc123"));
    }

    #[test]
    fn complex_2d_node_names_and_types() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes.len(), 6);

        assert_eq!(nodes[0].name(), "World");
        assert_eq!(nodes[0].class_name(), "Node2D");

        assert_eq!(nodes[1].name(), "Player");
        assert_eq!(nodes[1].class_name(), "CharacterBody2D");

        assert_eq!(nodes[2].name(), "CollisionShape");
        assert_eq!(nodes[2].class_name(), "CollisionShape2D");

        assert_eq!(nodes[3].name(), "Sprite");
        assert_eq!(nodes[3].class_name(), "Sprite2D");

        assert_eq!(nodes[4].name(), "Enemy");
        assert_eq!(nodes[4].class_name(), "CharacterBody2D");

        assert_eq!(nodes[5].name(), "EnemyCollision");
        assert_eq!(nodes[5].class_name(), "CollisionShape2D");
    }

    #[test]
    fn complex_2d_hierarchy() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        // World (root) has Player and Enemy as children.
        assert!(nodes[0].parent().is_none());
        assert_eq!(nodes[0].children().len(), 2);

        // Player is child of World.
        assert_eq!(nodes[1].parent(), Some(nodes[0].id()));
        // Player has CollisionShape and Sprite children.
        assert_eq!(nodes[1].children().len(), 2);

        // CollisionShape is child of Player.
        assert_eq!(nodes[2].parent(), Some(nodes[1].id()));

        // Sprite is child of Player.
        assert_eq!(nodes[3].parent(), Some(nodes[1].id()));

        // Enemy is child of World.
        assert_eq!(nodes[4].parent(), Some(nodes[0].id()));
        assert_eq!(nodes[4].children().len(), 1);

        // EnemyCollision is child of Enemy.
        assert_eq!(nodes[5].parent(), Some(nodes[4].id()));
    }

    #[test]
    fn complex_2d_player_groups() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        assert!(nodes[1].is_in_group("player"));
        assert!(nodes[1].is_in_group("damageable"));
        assert!(!nodes[1].is_in_group("enemy"));
    }

    #[test]
    fn complex_2d_enemy_groups() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        assert!(nodes[4].is_in_group("enemy"));
        assert!(nodes[4].is_in_group("damageable"));
        assert!(!nodes[4].is_in_group("player"));
    }

    #[test]
    fn complex_2d_player_position() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(
            nodes[1].get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );
    }

    #[test]
    fn complex_2d_enemy_position() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(
            nodes[4].get_property("position"),
            Variant::Vector2(Vector2::new(400.0, 200.0))
        );
    }

    #[test]
    fn complex_2d_script_property() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        // Player should have script resolved to its path.
        assert_eq!(
            nodes[1].get_property("_script_path"),
            Variant::String("res://player.gd".into())
        );
    }

    #[test]
    fn complex_2d_subresource_references() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        // CollisionShape's shape = SubResource("RectangleShape2D_abc")
        // This is parsed by parse_variant_value into "SubResource:RectangleShape2D_abc".
        assert_eq!(
            nodes[2].get_property("shape"),
            Variant::String("SubResource:RectangleShape2D_abc".into())
        );

        // EnemyCollision's shape = SubResource("CircleShape2D_def")
        assert_eq!(
            nodes[5].get_property("shape"),
            Variant::String("SubResource:CircleShape2D_def".into())
        );
    }

    #[test]
    fn complex_2d_ext_resource_ids_with_underscores() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();

        // Verify ext_resources were parsed with underscore IDs.
        assert_eq!(scene.ext_resources.len(), 2);
        assert!(scene.ext_resources.contains_key("1_abc"));
        assert!(scene.ext_resources.contains_key("2_xyz"));

        assert_eq!(scene.ext_resources["1_abc"].res_type, "Script");
        assert_eq!(scene.ext_resources["1_abc"].path, "res://player.gd");
        assert_eq!(scene.ext_resources["2_xyz"].res_type, "Texture2D");
        assert_eq!(scene.ext_resources["2_xyz"].path, "res://icon.svg");
    }

    #[test]
    fn complex_2d_metadata_slash_property() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        // metadata/custom_tag = "hero" — key has a slash.
        assert_eq!(
            nodes[1].get_property("metadata/custom_tag"),
            Variant::String("hero".into())
        );
    }

    #[test]
    fn complex_2d_sprite_offset() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(
            nodes[3].get_property("offset"),
            Variant::Vector2(Vector2::new(0.0, -16.0))
        );
    }

    #[test]
    fn complex_2d_connection() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        assert_eq!(scene.connection_count(), 1);

        let conn = &scene.connections()[0];
        assert_eq!(conn.signal_name, "body_entered");
        assert_eq!(conn.from_path, "Player");
        assert_eq!(conn.to_path, ".");
        assert_eq!(conn.method_name, "_on_player_body_entered");
    }

    #[test]
    fn complex_2d_add_to_tree() {
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        // 1 tree root + 6 scene nodes = 7 total.
        assert_eq!(tree.node_count(), 7);

        // Verify nested paths work.
        let collision = tree
            .get_node_by_path("/root/World/Player/CollisionShape")
            .unwrap();
        let collision_node = tree.get_node(collision).unwrap();
        assert_eq!(collision_node.class_name(), "CollisionShape2D");

        // Verify connection was wired.
        let player_id = tree.get_node_by_path("/root/World/Player").unwrap();
        let store = tree
            .signal_store(player_id)
            .expect("Player should have signal store");
        let sig = store.get_signal("body_entered").unwrap();
        assert_eq!(sig.connection_count(), 1);
        assert_eq!(sig.connections()[0].target_id, scene_root.object_id());
    }

    // -----------------------------------------------------------------------
    // UI with theme fixture tests
    // -----------------------------------------------------------------------

    #[test]
    fn ui_theme_parses_successfully() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        // MainMenu, Background, VBox, Title, PlayButton, QuitButton, Version = 7 nodes.
        assert_eq!(scene.node_count(), 7);
        assert_eq!(scene.uid.as_deref(), Some("uid://ui_theme_test"));
    }

    #[test]
    fn ui_theme_node_names() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(nodes[0].name(), "MainMenu");
        assert_eq!(nodes[1].name(), "Background");
        assert_eq!(nodes[2].name(), "VBox");
        assert_eq!(nodes[3].name(), "Title");
        assert_eq!(nodes[4].name(), "PlayButton");
        assert_eq!(nodes[5].name(), "QuitButton");
        assert_eq!(nodes[6].name(), "Version");
    }

    #[test]
    fn ui_theme_anchor_properties() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        // MainMenu: anchor_right = 1.0, anchor_bottom = 1.0
        assert_eq!(nodes[0].get_property("anchor_right"), Variant::Float(1.0));
        assert_eq!(nodes[0].get_property("anchor_bottom"), Variant::Float(1.0));

        // VBox has fractional anchors.
        assert_eq!(nodes[2].get_property("anchor_left"), Variant::Float(0.5));
        assert_eq!(nodes[2].get_property("anchor_top"), Variant::Float(0.3));
    }

    #[test]
    fn ui_theme_offset_properties() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        // VBox offsets.
        assert_eq!(nodes[2].get_property("offset_left"), Variant::Float(-150.0));
        assert_eq!(nodes[2].get_property("offset_right"), Variant::Float(150.0));
    }

    #[test]
    fn ui_theme_text_properties() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(
            nodes[3].get_property("text"),
            Variant::String("My Game".into())
        );
        assert_eq!(
            nodes[4].get_property("text"),
            Variant::String("Play".into())
        );
        assert_eq!(
            nodes[5].get_property("text"),
            Variant::String("Quit".into())
        );
        assert_eq!(
            nodes[6].get_property("text"),
            Variant::String("v1.0.0".into())
        );
    }

    #[test]
    fn ui_theme_override_slash_properties() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        // theme_override_styles/panel = SubResource("StyleBoxFlat_panel")
        assert_eq!(
            nodes[1].get_property("theme_override_styles/panel"),
            Variant::String("SubResource:StyleBoxFlat_panel".into())
        );

        // theme_override_font_sizes/font_size = 48
        assert_eq!(
            nodes[3].get_property("theme_override_font_sizes/font_size"),
            Variant::Int(48)
        );

        // theme_override_colors/font_color = Color(1, 1, 1, 1)
        assert_eq!(
            nodes[4].get_property("theme_override_colors/font_color"),
            Variant::Color(gdcore::math::Color::new(1.0, 1.0, 1.0, 1.0))
        );
    }

    #[test]
    fn ui_theme_horizontal_alignment() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(
            nodes[3].get_property("horizontal_alignment"),
            Variant::Int(1)
        );
    }

    #[test]
    fn ui_theme_metadata_bool() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        // Version label: metadata/auto_update = true
        assert_eq!(
            nodes[6].get_property("metadata/auto_update"),
            Variant::Bool(true)
        );
    }

    #[test]
    fn ui_theme_groups() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        assert!(nodes[6].is_in_group("ui_labels"));
        // Other nodes should not be in that group.
        assert!(!nodes[0].is_in_group("ui_labels"));
    }

    #[test]
    fn ui_theme_connections() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        assert_eq!(scene.connection_count(), 2);

        assert_eq!(scene.connections()[0].signal_name, "pressed");
        assert_eq!(scene.connections()[0].from_path, "VBox/PlayButton");
        assert_eq!(scene.connections()[0].to_path, ".");
        assert_eq!(scene.connections()[0].method_name, "_on_play_pressed");

        assert_eq!(scene.connections()[1].signal_name, "pressed");
        assert_eq!(scene.connections()[1].from_path, "VBox/QuitButton");
        assert_eq!(scene.connections()[1].method_name, "_on_quit_pressed");
    }

    #[test]
    fn ui_theme_script_resolved() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let nodes = scene.instance().unwrap();

        assert_eq!(
            nodes[0].get_property("_script_path"),
            Variant::String("res://ui/main_menu.gd".into())
        );
    }

    #[test]
    fn ui_theme_add_to_tree() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        // 1 tree root + 7 scene nodes = 8.
        assert_eq!(tree.node_count(), 8);

        // Deep path works.
        let play = tree
            .get_node_by_path("/root/MainMenu/VBox/PlayButton")
            .unwrap();
        let play_node = tree.get_node(play).unwrap();
        assert_eq!(play_node.class_name(), "Button");

        // Connections wired.
        let store = tree
            .signal_store(play)
            .expect("PlayButton should have signal store");
        let sig = store.get_signal("pressed").unwrap();
        assert_eq!(sig.connection_count(), 1);
        assert_eq!(sig.connections()[0].target_id, scene_root.object_id());
    }

    #[test]
    fn complex_2d_load_steps_ignored_gracefully() {
        // The load_steps=5 attribute should be parsed but ignored.
        // The scene should still parse without errors.
        let scene = PackedScene::from_tscn(COMPLEX_2D).unwrap();
        assert!(scene.node_count() > 0);
    }

    #[test]
    fn ui_theme_load_steps_ignored_gracefully() {
        let scene = PackedScene::from_tscn(UI_WITH_THEME).unwrap();
        assert!(scene.node_count() > 0);
    }

    // -- Property tests: only explicit tscn properties stored on nodes -----

    #[test]
    fn unset_properties_return_nil() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();
        let sprite = &nodes[2]; // Sprite2D with no explicit properties
        assert_eq!(sprite.class_name(), "Sprite2D");
        assert_eq!(sprite.get_property("position"), Variant::Nil);
        assert_eq!(sprite.get_property("rotation"), Variant::Nil);
        assert_eq!(sprite.get_property("scale"), Variant::Nil);
        assert_eq!(sprite.get_property("visible"), Variant::Nil);
    }

    #[test]
    fn explicit_position_preserved() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();
        let player = &nodes[1];
        assert_eq!(
            player.get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );
        assert_eq!(player.get_property("rotation"), Variant::Nil);
        assert_eq!(player.get_property("scale"), Variant::Nil);
    }

    #[test]
    fn only_explicit_properties_on_characterbody2d() {
        let tscn = "[gd_scene format=3]

[node name=\"Root\" type=\"Node\"]

[node name=\"Enemy\" type=\"CharacterBody2D\" parent=\".\"]
position = Vector2(400, 200)
";
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let enemy = &nodes[1];
        assert_eq!(enemy.class_name(), "CharacterBody2D");
        assert_eq!(
            enemy.get_property("position"),
            Variant::Vector2(Vector2::new(400.0, 200.0))
        );
        assert_eq!(enemy.get_property("rotation"), Variant::Nil);
        assert_eq!(enemy.get_property("scale"), Variant::Nil);
        assert_eq!(enemy.get_property("visible"), Variant::Nil);
    }

    #[test]
    fn node_class_has_no_implicit_properties() {
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let nodes = scene.instance().unwrap();
        let root = &nodes[0];
        assert_eq!(root.class_name(), "Node");
        assert_eq!(root.get_property("position"), Variant::Nil);
        assert_eq!(root.get_property("rotation"), Variant::Nil);
    }
}
