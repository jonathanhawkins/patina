//! Scene serialization — save a [`SceneTree`] subtree back to `.tscn` format.
//!
//! [`TscnSaver`] walks a node hierarchy depth-first and produces the text
//! representation that [`PackedScene::from_tscn`](crate::packed_scene::PackedScene::from_tscn)
//! can round-trip.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

// ---------------------------------------------------------------------------
// Variant formatting
// ---------------------------------------------------------------------------

/// Formats a [`Variant`] value in `.tscn` property syntax.
///
/// Handles all types that the parser can read back: scalars, strings,
/// math types, node paths, arrays, and dictionaries.
pub fn format_variant_value(v: &Variant) -> String {
    match v {
        Variant::Nil => "null".to_string(),
        Variant::Bool(b) => b.to_string(),
        Variant::Int(i) => i.to_string(),
        Variant::Float(f) => format_float(*f),
        Variant::String(s) => {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            format!("\"{escaped}\"")
        }
        Variant::StringName(sn) => {
            format!("&\"{}\"", sn.as_str())
        }
        Variant::NodePath(np) => {
            format!("NodePath(\"{np}\")")
        }
        Variant::Vector2(v) => {
            format!("Vector2({}, {})", format_f32(v.x), format_f32(v.y))
        }
        Variant::Vector3(v) => {
            format!(
                "Vector3({}, {}, {})",
                format_f32(v.x),
                format_f32(v.y),
                format_f32(v.z)
            )
        }
        Variant::Color(c) => {
            format!(
                "Color({}, {}, {}, {})",
                format_f32(c.r),
                format_f32(c.g),
                format_f32(c.b),
                format_f32(c.a)
            )
        }
        Variant::Rect2(r) => {
            format!(
                "Rect2({}, {}, {}, {})",
                format_f32(r.position.x),
                format_f32(r.position.y),
                format_f32(r.size.x),
                format_f32(r.size.y)
            )
        }
        Variant::Transform2D(t) => {
            format!(
                "Transform2D({}, {}, {}, {}, {}, {})",
                format_f32(t.x.x),
                format_f32(t.x.y),
                format_f32(t.y.x),
                format_f32(t.y.y),
                format_f32(t.origin.x),
                format_f32(t.origin.y)
            )
        }
        Variant::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_variant_value).collect();
            format!("[{}]", items.join(", "))
        }
        Variant::Dictionary(dict) => {
            let mut keys: Vec<&String> = dict.keys().collect();
            keys.sort();
            let pairs: Vec<String> = keys
                .iter()
                .map(|k| format!("\"{}\": {}", k, format_variant_value(&dict[*k])))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        // Types that don't have a standard .tscn text representation.
        other => format!("{other}"),
    }
}

/// Formats an f64 without unnecessary trailing zeros, but always with
/// a decimal point to distinguish from integers.
fn format_float(f: f64) -> String {
    if f.fract() == 0.0 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// Formats an f32 cleanly — whole numbers have no decimal point.
fn format_f32(f: f32) -> String {
    if f.fract() == 0.0 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}

// ---------------------------------------------------------------------------
// TscnSaver
// ---------------------------------------------------------------------------

/// Saves a [`SceneTree`] subtree as `.tscn` text.
pub struct TscnSaver;

impl TscnSaver {
    /// Serializes the subtree rooted at `root_id` to `.tscn` format.
    ///
    /// The root node is written without a `parent` attribute. All
    /// descendants get a relative `parent` path from the subtree root.
    /// Signal connections from the tree's signal stores are appended as
    /// `[connection]` sections.
    pub fn save_tree(tree: &SceneTree, root_id: NodeId) -> String {
        let mut out = String::new();
        writeln!(out, "[gd_scene format=3]").unwrap();

        // Collect subtree in depth-first (top-down) order.
        let mut node_ids = Vec::new();
        tree.collect_subtree_top_down(root_id, &mut node_ids);

        // Build a map from NodeId -> relative path within the subtree.
        let mut id_to_path: HashMap<NodeId, String> = HashMap::new();
        id_to_path.insert(root_id, ".".to_string());

        for &nid in &node_ids {
            let node = match tree.get_node(nid) {
                Some(n) => n,
                None => continue,
            };

            if nid == root_id {
                // Root node — no parent attribute.
                write_node_section(&mut out, node, None);
            } else {
                // Compute parent path.
                let parent_id = match node.parent() {
                    Some(pid) => pid,
                    None => continue,
                };
                let parent_path = id_to_path.get(&parent_id).cloned().unwrap_or_default();

                // Compute this node's path for its children.
                let node_path = if parent_path == "." {
                    node.name().to_string()
                } else {
                    format!("{}/{}", parent_path, node.name())
                };
                id_to_path.insert(nid, node_path);

                write_node_section(&mut out, node, Some(&parent_path));
            }
        }

        // Write connections from signal stores.
        write_connections(&mut out, tree, root_id, &node_ids, &id_to_path);

        out
    }

    /// Serializes only a specific subtree (not necessarily the scene root).
    ///
    /// This is equivalent to [`save_tree`](Self::save_tree) — the given
    /// `subtree_root` is treated as the root of the output `.tscn`.
    pub fn save_subtree(tree: &SceneTree, subtree_root: NodeId) -> String {
        Self::save_tree(tree, subtree_root)
    }
}

/// Writes a `[node ...]` section for a single node.
fn write_node_section(out: &mut String, node: &crate::node::Node, parent_path: Option<&str>) {
    out.push('\n');

    let name = node.name();
    let class = node.class_name();

    match parent_path {
        None => {
            writeln!(out, "[node name=\"{name}\" type=\"{class}\"]").unwrap();
        }
        Some(parent) => {
            writeln!(
                out,
                "[node name=\"{name}\" type=\"{class}\" parent=\"{parent}\"]"
            )
            .unwrap();
        }
    }

    // Write properties sorted for determinism.
    let mut props: Vec<(&String, &Variant)> = node.properties().collect();
    props.sort_by_key(|(k, _)| k.as_str().to_owned());
    for (key, value) in props {
        writeln!(out, "{} = {}", key, format_variant_value(value)).unwrap();
    }
}

/// Writes `[connection]` sections for all signals in the subtree.
fn write_connections(
    out: &mut String,
    tree: &SceneTree,
    _root_id: NodeId,
    node_ids: &[NodeId],
    id_to_path: &HashMap<NodeId, String>,
) {
    // Build reverse lookup: ObjectId -> NodeId for resolving connection targets.
    let mut object_id_to_node_id: HashMap<gdcore::ObjectId, NodeId> = HashMap::new();
    for &nid in node_ids {
        if let Some(node) = tree.get_node(nid) {
            let _ = node; // we need nid.object_id()
            object_id_to_node_id.insert(nid.object_id(), nid);
        }
    }

    // Collect connections in a deterministic order.
    struct ConnInfo {
        signal_name: String,
        from_path: String,
        to_path: String,
        method: String,
    }

    let mut conn_infos: Vec<ConnInfo> = Vec::new();

    // Iterate nodes in tree order for deterministic output.
    for &nid in node_ids {
        let store = match tree.signal_store(nid) {
            Some(s) => s,
            None => continue,
        };

        let from_path = match id_to_path.get(&nid) {
            Some(p) => p.clone(),
            None => continue,
        };

        let mut signal_names: Vec<&str> = store.signal_names();
        signal_names.sort();

        for sig_name in signal_names {
            let signal = match store.get_signal(sig_name) {
                Some(s) => s,
                None => continue,
            };

            for conn in signal.connections() {
                // Resolve target ObjectId -> NodeId -> path.
                let to_path = object_id_to_node_id
                    .get(&conn.target_id)
                    .and_then(|nid| id_to_path.get(nid))
                    .cloned()
                    .unwrap_or_else(|| ".".to_string());

                conn_infos.push(ConnInfo {
                    signal_name: sig_name.to_string(),
                    from_path: from_path.clone(),
                    to_path,
                    method: conn.method.clone(),
                });
            }
        }
    }

    for info in &conn_infos {
        writeln!(
            out,
            "\n[connection signal=\"{}\" from=\"{}\" to=\"{}\" method=\"{}\"]",
            info.signal_name, info.from_path, info.to_path, info.method,
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Node;
    use crate::packed_scene::{add_packed_scene_to_tree, PackedScene};
    use crate::scene_tree::SceneTree;
    use gdcore::math::{Color, Rect2, Transform2D, Vector2, Vector3};
    use gdcore::node_path::NodePath;
    use gdobject::signal::Connection;

    // -- format_variant_value tests -----------------------------------------

    #[test]
    fn format_nil() {
        assert_eq!(format_variant_value(&Variant::Nil), "null");
    }

    #[test]
    fn format_bool() {
        assert_eq!(format_variant_value(&Variant::Bool(true)), "true");
        assert_eq!(format_variant_value(&Variant::Bool(false)), "false");
    }

    #[test]
    fn format_int() {
        assert_eq!(format_variant_value(&Variant::Int(42)), "42");
        assert_eq!(format_variant_value(&Variant::Int(-7)), "-7");
    }

    #[test]
    fn format_float() {
        assert_eq!(format_variant_value(&Variant::Float(3.14)), "3.14");
        assert_eq!(format_variant_value(&Variant::Float(5.0)), "5.0");
    }

    #[test]
    fn format_string() {
        assert_eq!(
            format_variant_value(&Variant::String("hello".into())),
            "\"hello\""
        );
    }

    #[test]
    fn format_string_with_escapes() {
        assert_eq!(
            format_variant_value(&Variant::String("line1\nline2\ttab".into())),
            r#""line1\nline2\ttab""#
        );
    }

    #[test]
    fn format_vector2() {
        let v = Vector2::new(10.0, 20.5);
        assert_eq!(
            format_variant_value(&Variant::Vector2(v)),
            "Vector2(10, 20.5)"
        );
    }

    #[test]
    fn format_vector3() {
        let v = Vector3::new(1.0, 2.0, 3.5);
        assert_eq!(
            format_variant_value(&Variant::Vector3(v)),
            "Vector3(1, 2, 3.5)"
        );
    }

    #[test]
    fn format_color() {
        let c = Color::new(0.5, 0.6, 0.7, 1.0);
        assert_eq!(
            format_variant_value(&Variant::Color(c)),
            "Color(0.5, 0.6, 0.7, 1)"
        );
    }

    #[test]
    fn format_rect2() {
        let r = Rect2::new(Vector2::new(1.0, 2.0), Vector2::new(3.0, 4.0));
        assert_eq!(
            format_variant_value(&Variant::Rect2(r)),
            "Rect2(1, 2, 3, 4)"
        );
    }

    #[test]
    fn format_transform2d() {
        let t = Transform2D::IDENTITY;
        assert_eq!(
            format_variant_value(&Variant::Transform2D(t)),
            "Transform2D(1, 0, 0, 1, 0, 0)"
        );
    }

    #[test]
    fn format_node_path() {
        let np = NodePath::new("/root/Player");
        assert_eq!(
            format_variant_value(&Variant::NodePath(np)),
            "NodePath(\"/root/Player\")"
        );
    }

    #[test]
    fn format_array() {
        let arr = Variant::Array(vec![
            Variant::Int(1),
            Variant::String("two".into()),
            Variant::Bool(true),
        ]);
        assert_eq!(format_variant_value(&arr), "[1, \"two\", true]");
    }

    #[test]
    fn format_empty_array() {
        assert_eq!(format_variant_value(&Variant::Array(vec![])), "[]");
    }

    #[test]
    fn format_dictionary() {
        let mut d = std::collections::HashMap::new();
        d.insert("b".to_string(), Variant::Int(2));
        d.insert("a".to_string(), Variant::Int(1));
        let result = format_variant_value(&Variant::Dictionary(d));
        // Keys are sorted.
        assert_eq!(result, "{\"a\": 1, \"b\": 2}");
    }

    #[test]
    fn format_empty_dictionary() {
        assert_eq!(
            format_variant_value(&Variant::Dictionary(std::collections::HashMap::new())),
            "{}"
        );
    }

    // -- TscnSaver tests ----------------------------------------------------

    #[test]
    fn save_empty_scene() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = Node::new("Main", "Node");
        let scene_root_id = tree.add_child(root, scene_root).unwrap();

        let output = TscnSaver::save_tree(&tree, scene_root_id);
        assert!(output.contains("[gd_scene format=3]"));
        assert!(output.contains("[node name=\"Main\" type=\"Node\"]"));
        // Root node should have no parent attribute.
        assert!(!output.contains("parent="));
    }

    #[test]
    fn save_with_properties() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("Player", "Node2D");
        node.set_property("position", Variant::Vector2(Vector2::new(100.0, 200.0)));
        node.set_property("health", Variant::Int(100));
        let nid = tree.add_child(root, node).unwrap();

        let output = TscnSaver::save_tree(&tree, nid);
        assert!(output.contains("position = Vector2(100, 200)"));
        assert!(output.contains("health = 100"));
    }

    #[test]
    fn save_with_nested_children() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root = Node::new("World", "Node");
        let world_id = tree.add_child(root, scene_root).unwrap();

        let level = Node::new("Level", "Node");
        let level_id = tree.add_child(world_id, level).unwrap();

        let enemies = Node::new("Enemies", "Node");
        let enemies_id = tree.add_child(level_id, enemies).unwrap();

        let mut boss = Node::new("Boss", "Node2D");
        boss.set_property("health", Variant::Int(500));
        tree.add_child(enemies_id, boss).unwrap();

        let output = TscnSaver::save_tree(&tree, world_id);

        assert!(output.contains("[node name=\"World\" type=\"Node\"]"));
        assert!(output.contains("[node name=\"Level\" type=\"Node\" parent=\".\"]"));
        assert!(output.contains("[node name=\"Enemies\" type=\"Node\" parent=\"Level\"]"));
        assert!(output.contains("[node name=\"Boss\" type=\"Node2D\" parent=\"Level/Enemies\"]"));
        assert!(output.contains("health = 500"));
    }

    #[test]
    fn save_with_signals() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root = Node::new("Root", "Control");
        let root_id = tree.add_child(root, scene_root).unwrap();

        let button = Node::new("Button", "Button");
        let button_id = tree.add_child(root_id, button).unwrap();

        // Connect button "pressed" signal to root.
        let conn = Connection::new(root_id.object_id(), "_on_button_pressed");
        tree.connect_signal(button_id, "pressed", conn);

        let output = TscnSaver::save_tree(&tree, root_id);

        assert!(output.contains(
            "[connection signal=\"pressed\" from=\"Button\" to=\".\" method=\"_on_button_pressed\"]"
        ));
    }

    #[test]
    fn save_subtree_is_same_as_save_tree() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Sub", "Node2D");
        let nid = tree.add_child(root, node).unwrap();

        let a = TscnSaver::save_tree(&tree, nid);
        let b = TscnSaver::save_subtree(&tree, nid);
        assert_eq!(a, b);
    }

    #[test]
    fn roundtrip_simple_scene() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Player" type="Node2D" parent="."]
position = Vector2(100, 200)

[node name="Sprite" type="Sprite2D" parent="Player"]
"#;
        // Parse -> instance -> add to tree -> save -> re-parse.
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);

        // Re-parse the saved output.
        let scene2 = PackedScene::from_tscn(&saved).unwrap();
        assert_eq!(scene2.node_count(), 3);

        // Instance and verify structure.
        let nodes2 = scene2.instance().unwrap();
        assert_eq!(nodes2[0].name(), "Root");
        assert_eq!(nodes2[0].class_name(), "Node");
        assert_eq!(nodes2[1].name(), "Player");
        assert_eq!(nodes2[1].class_name(), "Node2D");
        assert_eq!(
            nodes2[1].get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );
        assert_eq!(nodes2[2].name(), "Sprite");
        assert_eq!(nodes2[2].class_name(), "Sprite2D");
    }

    #[test]
    fn roundtrip_with_connections() {
        let tscn = r#"
[gd_scene format=3]

[node name="Root" type="Control"]

[node name="Button" type="Button" parent="."]

[connection signal="pressed" from="Button" to="." method="_on_pressed"]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);

        // Verify connection section is present.
        assert!(saved.contains("[connection signal=\"pressed\""));
        assert!(saved.contains("from=\"Button\""));
        assert!(saved.contains("to=\".\""));
        assert!(saved.contains("method=\"_on_pressed\""));

        // Re-parse.
        let scene2 = PackedScene::from_tscn(&saved).unwrap();
        assert_eq!(scene2.connection_count(), 1);
    }

    #[test]
    fn save_single_root_node() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let node = Node::new("Alone", "Node");
        let nid = tree.add_child(root, node).unwrap();

        let output = TscnSaver::save_tree(&tree, nid);
        assert!(output.contains("[gd_scene format=3]"));
        assert!(output.contains("[node name=\"Alone\" type=\"Node\"]"));
        // Should have no connection sections.
        assert!(!output.contains("[connection"));
    }

    #[test]
    fn properties_are_sorted() {
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let mut node = Node::new("N", "Node");
        node.set_property("z_prop", Variant::Int(3));
        node.set_property("a_prop", Variant::Int(1));
        node.set_property("m_prop", Variant::Int(2));
        let nid = tree.add_child(root, node).unwrap();

        let output = TscnSaver::save_tree(&tree, nid);

        let a_pos = output.find("a_prop").unwrap();
        let m_pos = output.find("m_prop").unwrap();
        let z_pos = output.find("z_prop").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn roundtrip_deep_nesting() {
        let tscn = r#"
[gd_scene format=3]

[node name="World" type="Node"]

[node name="Level" type="Node" parent="."]

[node name="Enemies" type="Node" parent="Level"]

[node name="Boss" type="Node2D" parent="Level/Enemies"]
health = 100
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);
        let scene2 = PackedScene::from_tscn(&saved).unwrap();
        assert_eq!(scene2.node_count(), 4);

        let nodes2 = scene2.instance().unwrap();
        assert_eq!(nodes2[3].name(), "Boss");
        assert_eq!(nodes2[3].get_property("health"), Variant::Int(100));
    }
}
