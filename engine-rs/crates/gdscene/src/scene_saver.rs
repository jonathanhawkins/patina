//! Scene serialization — save a [`SceneTree`] subtree back to `.tscn` format.
//!
//! [`TscnSaver`] walks a node hierarchy depth-first and produces the text
//! representation that [`PackedScene::from_tscn`](crate::packed_scene::PackedScene::from_tscn)
//! can round-trip.

use std::collections::{BTreeMap, HashMap};
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
/// Renders a dictionary key to its text form for `.tscn` output. String keys
/// use their raw text; other Variant kinds use their lossy string form.
fn dict_key_display(k: &Variant) -> String {
    match k {
        Variant::String(s) => s.clone(),
        other => other.to_string_lossy(),
    }
}

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
            // Keys are Variants; render each to a stable display string and sort
            // for deterministic output.
            let mut pairs: Vec<(String, String)> = dict
                .iter()
                .map(|(k, v)| (dict_key_display(k), format_variant_value(v)))
                .collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let pairs: Vec<String> = pairs
                .iter()
                .map(|(k, v)| format!("\"{}\": {}", k, v))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        // Packed arrays serialize as `PackedXArray(elem, elem, …)`, with
        // vector/color elements flattened into their components (Godot syntax).
        Variant::PackedByteArray(v) => {
            let items: Vec<String> = v.iter().map(|b| b.to_string()).collect();
            format!("PackedByteArray({})", items.join(", "))
        }
        Variant::PackedInt32Array(v) => {
            let items: Vec<String> = v.iter().map(|i| i.to_string()).collect();
            format!("PackedInt32Array({})", items.join(", "))
        }
        Variant::PackedInt64Array(v) => {
            let items: Vec<String> = v.iter().map(|i| i.to_string()).collect();
            format!("PackedInt64Array({})", items.join(", "))
        }
        Variant::PackedFloat32Array(v) => {
            let items: Vec<String> = v.iter().map(|f| format_f32(*f)).collect();
            format!("PackedFloat32Array({})", items.join(", "))
        }
        Variant::PackedFloat64Array(v) => {
            let items: Vec<String> = v.iter().map(|f| format_float(*f)).collect();
            format!("PackedFloat64Array({})", items.join(", "))
        }
        Variant::PackedStringArray(v) => {
            let items: Vec<String> = v
                .iter()
                .map(|s| {
                    let escaped = s
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                        .replace('\n', "\\n")
                        .replace('\t', "\\t");
                    format!("\"{escaped}\"")
                })
                .collect();
            format!("PackedStringArray({})", items.join(", "))
        }
        Variant::PackedVector2Array(v) => {
            let items: Vec<String> = v
                .iter()
                .flat_map(|p| [format_f32(p.x), format_f32(p.y)])
                .collect();
            format!("PackedVector2Array({})", items.join(", "))
        }
        Variant::PackedVector3Array(v) => {
            let items: Vec<String> = v
                .iter()
                .flat_map(|p| [format_f32(p.x), format_f32(p.y), format_f32(p.z)])
                .collect();
            format!("PackedVector3Array({})", items.join(", "))
        }
        Variant::PackedColorArray(v) => {
            let items: Vec<String> = v
                .iter()
                .flat_map(|c| {
                    [
                        format_f32(c.r),
                        format_f32(c.g),
                        format_f32(c.b),
                        format_f32(c.a),
                    ]
                })
                .collect();
            format!("PackedColorArray({})", items.join(", "))
        }
        // 3D math types serialize with their Godot constructor syntax, matching
        // the field order `gdresource::loader::parse_variant_value` reads back.
        Variant::Basis(b) => {
            format!(
                "Basis({}, {}, {}, {}, {}, {}, {}, {}, {})",
                format_f32(b.x.x),
                format_f32(b.x.y),
                format_f32(b.x.z),
                format_f32(b.y.x),
                format_f32(b.y.y),
                format_f32(b.y.z),
                format_f32(b.z.x),
                format_f32(b.z.y),
                format_f32(b.z.z),
            )
        }
        Variant::Transform3D(t) => {
            format!(
                "Transform3D({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                format_f32(t.basis.x.x),
                format_f32(t.basis.x.y),
                format_f32(t.basis.x.z),
                format_f32(t.basis.y.x),
                format_f32(t.basis.y.y),
                format_f32(t.basis.y.z),
                format_f32(t.basis.z.x),
                format_f32(t.basis.z.y),
                format_f32(t.basis.z.z),
                format_f32(t.origin.x),
                format_f32(t.origin.y),
                format_f32(t.origin.z),
            )
        }
        Variant::Quaternion(q) => {
            format!(
                "Quaternion({}, {}, {}, {})",
                format_f32(q.x),
                format_f32(q.y),
                format_f32(q.z),
                format_f32(q.w)
            )
        }
        Variant::Aabb(a) => {
            format!(
                "AABB({}, {}, {}, {}, {}, {})",
                format_f32(a.position.x),
                format_f32(a.position.y),
                format_f32(a.position.z),
                format_f32(a.size.x),
                format_f32(a.size.y),
                format_f32(a.size.z)
            )
        }
        Variant::Plane(p) => {
            format!(
                "Plane({}, {}, {}, {})",
                format_f32(p.normal.x),
                format_f32(p.normal.y),
                format_f32(p.normal.z),
                format_f32(p.d)
            )
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
        // Collect subtree in depth-first (top-down) order.
        let mut node_ids = Vec::new();
        tree.collect_subtree_top_down(root_id, &mut node_ids);

        // Reconstruct the external/inline resource tables from node references
        // and `__resref__`/`_instance_path`/`_script_path` companions so the
        // saved scene declares them the way Godot's text saver does.
        let tables = collect_resource_tables(tree, &node_ids);

        let mut out = String::new();
        // Header. Godot emits `load_steps` = (ext + sub resources) + 1 whenever
        // any resources exist; a resource-free scene keeps the bare header.
        let resource_count = tables.ext.len() + tables.sub.len();
        if resource_count > 0 {
            writeln!(out, "[gd_scene load_steps={} format=3]", resource_count + 1).unwrap();
        } else {
            writeln!(out, "[gd_scene format=3]").unwrap();
        }

        // External resource table.
        if !tables.ext.is_empty() {
            out.push('\n');
            for (id, res_type, path) in &tables.ext {
                writeln!(
                    out,
                    "[ext_resource type=\"{res_type}\" path=\"{path}\" id=\"{id}\"]"
                )
                .unwrap();
            }
        }
        // Inline sub-resource sections.
        for (id, class) in &tables.sub {
            writeln!(out, "\n[sub_resource type=\"{class}\" id=\"{id}\"]").unwrap();
        }

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

/// External and inline resource declarations reconstructed for a saved scene.
struct ResourceTables {
    /// External resources as `(id, res_type, path)`, ordered by id.
    ext: Vec<(String, String, String)>,
    /// Inline sub-resources as `(id, class)`, ordered by id.
    sub: Vec<(String, String)>,
}

/// Extracts the id from an `ExtResource("id")` reference string.
fn parse_ext_resource_id(s: &str) -> Option<String> {
    let inner = s.strip_prefix("ExtResource(")?.strip_suffix(')')?.trim();
    let inner = inner.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_string())
}

/// Property keys that are internal companions, never emitted as scene
/// properties (they are re-encoded as resource references / header attributes).
fn is_internal_prop(key: &str) -> bool {
    key.starts_with("__resref__")
        || key.starts_with("__viewport_")
        || key == "_instance"
        || key == "_instance_path"
        || key == "_script_path"
}

/// Walks the subtree and reconstructs the ext/sub resource tables from node
/// references. Script paths come from `_script_path`, instanced sub-scene paths
/// from `_instance_path`, and inline sub-resources from `__resref__<prop>`
/// companions (which carry `class::id`). Ids are preserved from the original
/// `ExtResource("id")` / `SubResource("id")` references so a save/load round-trip
/// is stable.
fn collect_resource_tables(tree: &SceneTree, node_ids: &[NodeId]) -> ResourceTables {
    let mut ext_by_id: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut sub_by_id: BTreeMap<String, String> = BTreeMap::new();

    for &nid in node_ids {
        let node = match tree.get_node(nid) {
            Some(n) => n,
            None => continue,
        };

        // Script external resource: raw `script = ExtResource("id")` reference
        // paired with the resolved `_script_path` companion.
        if let (Variant::String(script_ref), Variant::String(path)) = (
            node.get_property("script"),
            node.get_property("_script_path"),
        ) {
            if let Some(id) = parse_ext_resource_id(&script_ref) {
                ext_by_id.entry(id).or_insert(("Script".to_string(), path));
            }
        }

        // Instanced sub-scene external resource: `_instance` ref + resolved path.
        if let (Variant::String(inst_ref), Variant::String(path)) = (
            node.get_property("_instance"),
            node.get_property("_instance_path"),
        ) {
            if let Some(id) = parse_ext_resource_id(&inst_ref) {
                ext_by_id
                    .entry(id)
                    .or_insert(("PackedScene".to_string(), path));
            }
        }

        // Inline sub-resources referenced via `__resref__<prop>` = `class::id`.
        for (k, v) in node.properties() {
            if k.starts_with("__resref__") {
                if let Variant::String(meta) = v {
                    if let Some((class, id)) = meta.rsplit_once("::") {
                        if !id.is_empty() {
                            sub_by_id
                                .entry(id.to_string())
                                .or_insert_with(|| class.to_string());
                        }
                    }
                }
            }
        }
    }

    ResourceTables {
        ext: ext_by_id
            .into_iter()
            .map(|(id, (ty, path))| (id, ty, path))
            .collect(),
        sub: sub_by_id.into_iter().collect(),
    }
}

/// Writes a `[node ...]` section for a single node.
fn write_node_section(out: &mut String, node: &crate::node::Node, parent_path: Option<&str>) {
    out.push('\n');

    let name = node.name();
    let class = node.class_name();

    // Emit group membership in the node header (`groups=["a", "b"]`), matching
    // the loader's parse format, so groups survive a save/load round-trip.
    // Sorted for deterministic output.
    let groups_attr = {
        let mut groups: Vec<&str> = node.groups().iter().map(String::as_str).collect();
        if groups.is_empty() {
            String::new()
        } else {
            groups.sort_unstable();
            let list: Vec<String> = groups.iter().map(|g| format!("\"{g}\"")).collect();
            format!(" groups=[{}]", list.join(", "))
        }
    };

    // A node that instances a sub-scene is written `instance=ExtResource("id")`
    // with NO `type=` attribute, exactly as Godot does.
    let instance_attr = match node.get_property("_instance") {
        Variant::String(s) => {
            parse_ext_resource_id(&s).map(|id| format!(" instance=ExtResource(\"{id}\")"))
        }
        _ => None,
    };
    let type_attr = if instance_attr.is_some() {
        String::new()
    } else {
        format!(" type=\"{class}\"")
    };
    let parent_attr = match parent_path {
        None => String::new(),
        Some(parent) => format!(" parent=\"{parent}\""),
    };
    let instance_attr = instance_attr.unwrap_or_default();
    writeln!(
        out,
        "[node name=\"{name}\"{type_attr}{parent_attr}{groups_attr}{instance_attr}]"
    )
    .unwrap();

    // Map each property that references an inline sub-resource to its id, from
    // the `__resref__<prop>` companions.
    let mut sub_ref: HashMap<&str, String> = HashMap::new();
    for (k, v) in node.properties() {
        if let Some(prop) = k.strip_prefix("__resref__") {
            if let Variant::String(meta) = v {
                if let Some((_class, id)) = meta.rsplit_once("::") {
                    sub_ref.insert(prop, id.to_string());
                }
            }
        }
    }

    // Write properties sorted for determinism. Internal companion keys are
    // skipped — they were folded into the resource tables / header attributes.
    let mut props: Vec<(&String, &Variant)> = node
        .properties()
        .filter(|(k, _)| !is_internal_prop(k))
        .collect();
    props.sort_by_key(|(k, _)| k.as_str().to_owned());
    for (key, value) in props {
        // Property that references an inline sub-resource (mesh/shape/etc): emit
        // the canonical `SubResource("id")` regardless of any resolved marker.
        if let Some(id) = sub_ref.get(key.as_str()) {
            writeln!(out, "{key} = SubResource(\"{id}\")").unwrap();
            continue;
        }
        // Resource references stored as strings are re-encoded as their unquoted
        // constructor form (e.g. `script = ExtResource("id")`).
        if let Variant::String(s) = value {
            if let Some(id) = parse_ext_resource_id(s) {
                writeln!(out, "{key} = ExtResource(\"{id}\")").unwrap();
                continue;
            }
            if let Some(id) = s.strip_prefix("ExtResource:") {
                writeln!(out, "{key} = ExtResource(\"{id}\")").unwrap();
                continue;
            }
            if let Some(id) = s.strip_prefix("SubResource:") {
                writeln!(out, "{key} = SubResource(\"{id}\")").unwrap();
                continue;
            }
        }
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
    use gdcore::math3d::{Aabb, Basis, Plane, Quaternion, Transform3D};
    use gdcore::node_path::NodePath;
    use gdobject::signal::Connection;
    use gdvariant::Dictionary;

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
        let mut d = Dictionary::new();
        d.insert("b".to_string(), Variant::Int(2));
        d.insert("a".to_string(), Variant::Int(1));
        let result = format_variant_value(&Variant::Dictionary(d));
        // Keys are sorted.
        assert_eq!(result, "{\"a\": 1, \"b\": 2}");
    }

    #[test]
    fn format_empty_dictionary() {
        assert_eq!(
            format_variant_value(&Variant::Dictionary(Dictionary::new())),
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

    // -- Packed array .tscn round-trips (pat-u3l8c.11) ----------------------

    /// Format a packed value to `.tscn` text and parse it back; the packed
    /// type and contents must survive unchanged.
    fn packed_roundtrip(v: Variant) {
        let text = format_variant_value(&v);
        let parsed = gdresource::loader::parse_variant_value(&text)
            .unwrap_or_else(|e| panic!("re-parse of {text:?} failed: {e:?}"));
        assert_eq!(parsed, v, "round-trip changed value; text was {text:?}");
    }

    #[test]
    fn packed_arrays_keep_type_through_tscn_roundtrip() {
        packed_roundtrip(Variant::PackedByteArray(vec![0, 1, 2, 255]));
        packed_roundtrip(Variant::PackedByteArray(vec![]));
        packed_roundtrip(Variant::PackedInt32Array(vec![1, -2, 3]));
        packed_roundtrip(Variant::PackedInt64Array(vec![1, -2, 9_000_000_000]));
        packed_roundtrip(Variant::PackedFloat32Array(vec![0.0, 1.5, -3.25]));
        packed_roundtrip(Variant::PackedFloat64Array(vec![0.0, 1.5, -3.25]));
        packed_roundtrip(Variant::PackedStringArray(vec![
            "alpha".into(),
            "with space".into(),
        ]));
        packed_roundtrip(Variant::PackedVector2Array(vec![
            Vector2::new(1.0, 2.0),
            Vector2::new(-3.0, 4.0),
        ]));
        packed_roundtrip(Variant::PackedVector3Array(vec![Vector3::new(
            1.0, 2.0, 3.0,
        )]));
        packed_roundtrip(Variant::PackedColorArray(vec![Color::new(
            1.0, 0.0, 0.5, 1.0,
        )]));
    }

    #[test]
    fn packed_array_saved_as_constructor_syntax() {
        assert_eq!(
            format_variant_value(&Variant::PackedInt32Array(vec![1, 2, 3])),
            "PackedInt32Array(1, 2, 3)"
        );
        assert_eq!(
            format_variant_value(&Variant::PackedVector2Array(vec![Vector2::new(1.0, 2.0)])),
            "PackedVector2Array(1, 2)"
        );
        assert_eq!(
            format_variant_value(&Variant::PackedByteArray(vec![])),
            "PackedByteArray()"
        );
    }

    // -- 3D math type serialization (pat-nzgdu.18) --------------------------

    #[test]
    fn transform3d_saved_as_constructor_syntax() {
        let t = Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 0.0, 0.0),
                y: Vector3::new(0.0, 1.0, 0.0),
                z: Vector3::new(0.0, 0.0, 1.0),
            },
            origin: Vector3::new(5.0, 6.0, 7.0),
        });
        assert_eq!(
            format_variant_value(&t),
            "Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 6, 7)"
        );
    }

    #[test]
    fn math3d_types_keep_value_through_tscn_roundtrip() {
        packed_roundtrip(Variant::Transform3D(Transform3D {
            basis: Basis {
                x: Vector3::new(1.0, 2.0, 3.0),
                y: Vector3::new(4.0, 5.0, 6.0),
                z: Vector3::new(7.0, 8.0, 9.0),
            },
            origin: Vector3::new(-1.5, 0.0, 2.25),
        }));
        packed_roundtrip(Variant::Basis(Basis {
            x: Vector3::new(1.0, 0.0, 0.0),
            y: Vector3::new(0.0, 1.0, 0.0),
            z: Vector3::new(0.0, 0.0, 1.0),
        }));
        packed_roundtrip(Variant::Quaternion(Quaternion::new(0.0, 0.0, 0.0, 1.0)));
        packed_roundtrip(Variant::Aabb(Aabb::new(
            Vector3::new(0.0, 1.0, 2.0),
            Vector3::new(3.0, 4.0, 5.0),
        )));
        packed_roundtrip(Variant::Plane(Plane::new(Vector3::new(0.0, 1.0, 0.0), 5.0)));
    }

    // -- Lossless ext/sub resource round-trip (pat-nzgdu.18) ----------------

    /// A complex scene — root with a script `ext_resource`, a child instancing a
    /// sub-scene, a node with an inline `sub_resource`, and a `Transform3D`
    /// property — must survive a save/load round-trip with its resource tables,
    /// script path, instance ref, and Transform3D value intact.
    #[test]
    fn tscn_save_roundtrip_ext_and_sub_resources() {
        let tscn = r#"[gd_scene load_steps=4 format=3 uid="uid://testscene"]

[ext_resource type="Script" path="res://player.gd" id="1_script"]
[ext_resource type="PackedScene" path="res://child.tscn" id="2_scene"]

[sub_resource type="BoxShape3D" id="Box_1"]

[node name="Root" type="Node3D"]
script = ExtResource("1_script")
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 5, 6, 7)

[node name="Body" type="CollisionShape3D" parent="."]
shape = SubResource("Box_1")

[node name="Child" parent="." instance=ExtResource("2_scene")]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root_id = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let saved = TscnSaver::save_tree(&tree, scene_root_id);

        // Sanity: the saved text declares the reconstructed tables & header.
        assert!(
            saved.contains("[gd_scene load_steps=4 format=3]"),
            "missing header with load_steps:\n{saved}"
        );
        assert!(
            saved.contains(
                "[ext_resource type=\"Script\" path=\"res://player.gd\" id=\"1_script\"]"
            ),
            "missing script ext_resource:\n{saved}"
        );
        assert!(
            saved.contains(
                "[ext_resource type=\"PackedScene\" path=\"res://child.tscn\" id=\"2_scene\"]"
            ),
            "missing instanced-scene ext_resource:\n{saved}"
        );
        assert!(
            saved.contains("[sub_resource type=\"BoxShape3D\" id=\"Box_1\"]"),
            "missing sub_resource section:\n{saved}"
        );
        assert!(
            saved.contains("script = ExtResource(\"1_script\")"),
            "script should be an unquoted ExtResource ref:\n{saved}"
        );
        assert!(
            saved.contains("instance=ExtResource(\"2_scene\")"),
            "instanced child should carry an instance= attribute:\n{saved}"
        );
        assert!(
            saved.contains("shape = SubResource(\"Box_1\")"),
            "shape should reference the sub_resource:\n{saved}"
        );

        // Re-parse the saved text and diff the resource tables against the
        // original — the round-trip diff must be empty for these.
        let scene2 = PackedScene::from_tscn(&saved).unwrap();

        let ext1 = scene.ext_resources();
        let ext2 = scene2.ext_resources();
        assert_eq!(
            ext2.len(),
            ext1.len(),
            "ext_resource count changed:\n{saved}"
        );
        for (id, e1) in ext1 {
            let e2 = ext2
                .get(id)
                .unwrap_or_else(|| panic!("ext_resource {id} lost on round-trip:\n{saved}"));
            assert_eq!(e2.res_type, e1.res_type, "ext_resource {id} type changed");
            assert_eq!(e2.path, e1.path, "ext_resource {id} path changed");
        }

        assert_eq!(
            scene2.sub_resource_type("Box_1"),
            Some("BoxShape3D"),
            "sub_resource type lost on round-trip:\n{saved}"
        );

        // Script path, instance ref, and Transform3D value survive intact.
        let nodes1 = scene.instance().unwrap();
        let nodes2 = scene2.instance().unwrap();

        let root1 = &nodes1[0];
        let root2 = nodes2.iter().find(|n| n.name() == "Root").expect("Root");
        assert_eq!(
            root2.get_property("_script_path"),
            root1.get_property("_script_path"),
            "script path changed"
        );
        assert_eq!(
            root2.get_property("transform"),
            root1.get_property("transform"),
            "Transform3D value changed"
        );

        let child1 = nodes1.iter().find(|n| n.name() == "Child").expect("Child");
        let child2 = nodes2.iter().find(|n| n.name() == "Child").expect("Child");
        assert_eq!(
            child2.get_property("_instance"),
            child1.get_property("_instance"),
            "instance ref changed"
        );
    }
}
