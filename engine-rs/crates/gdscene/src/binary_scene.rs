//! Binary `PackedScene` (`.scn`) support (pat-33wmr).
//!
//! A binary scene is a binary resource (`RSRC`, usually `RSCC`-compressed)
//! whose main resource is a `PackedScene` holding Godot's `_bundled`
//! dictionary — the `SceneState` in its serialized form
//! (`scene/resources/packed_scene.cpp`, `SceneState::set_bundled_scene`).
//!
//! Rather than build a second instancing path beside the `.tscn` one, this
//! module converts the bundle into the `.tscn` text Godot's own text saver
//! would write for the same `SceneState` (`resource_format_text.cpp`), and
//! hands that to [`PackedScene::from_tscn`]. Every behaviour the text path
//! has — ext/sub-resource resolution, material flattening, GridMap
//! `mesh_library` companions, `instance=` sub-scenes, inherited scenes,
//! connections — therefore applies to a binary scene unchanged, and the two
//! formats cannot drift.
//!
//! The official 3D platformer keeps its entire level in `stage/grid_map.scn`;
//! before this, every reader `read_to_string`-ed the file, failed on the
//! compressed bytes, and the instanced GridMap came up as an empty `Node`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use gdcore::error::{EngineError, EngineResult};
use gdresource::Resource;
use gdvariant::Variant;

use crate::packed_scene::PackedScene;

// SceneState flag constants (`scene/resources/packed_scene.h`).
const NO_PARENT_SAVED: i32 = 0x7FFF_FFFF;
const NAME_INDEX_BITS: u32 = 18;
const NAME_MASK: u32 = (1 << NAME_INDEX_BITS) - 1;
const FLAG_ID_IS_PATH: i32 = 1 << 30;
const TYPE_INSTANTIATED: i32 = 0x7FFF_FFFF;
const FLAG_INSTANCE_IS_PLACEHOLDER: i32 = 1 << 30;
const FLAG_PATH_PROPERTY_IS_NODE: i32 = 1 << 30;
const FLAG_PROP_NAME_MASK: i32 = FLAG_PATH_PROPERTY_IS_NODE - 1;
const FLAG_MASK: i32 = (1 << 24) - 1;
/// `Object::CONNECT_PERSIST`, which the text saver leaves implicit.
const CONNECT_PERSIST: i32 = 2;

/// Whether `bytes` hold a binary resource (`RSRC` or `RSCC`) rather than text.
pub fn is_binary_scene(bytes: &[u8]) -> bool {
    gdresource::res_loader::is_binary_resource(bytes)
}

/// Returns `.tscn` source for a scene file's bytes in either format: text is
/// passed through (it must be UTF-8), a binary scene is converted.
pub fn scene_source_from_bytes(bytes: &[u8], path: &str) -> EngineResult<String> {
    if is_binary_scene(bytes) {
        let resource = gdresource::res_loader::load_res_binary(bytes, path)?;
        return binary_scene_to_tscn(&resource);
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|e| EngineError::Parse(format!("{path} is not a binary scene or UTF-8: {e}")))
}

/// Reads a scene file (`res://`, absolute, or relative) from the mounted packs
/// or the filesystem and returns its `.tscn` source, converting a binary
/// `.scn`. `None` when it cannot be read or decoded.
pub fn read_scene_source(path: &str) -> Option<String> {
    let bytes = match gdcore::file_access::read_from_packs(path) {
        Some(bytes) => bytes,
        None => std::fs::read(gdcore::resolve_path(path).ok()?).ok()?,
    };
    match scene_source_from_bytes(&bytes, path) {
        Ok(text) => Some(text),
        Err(e) => {
            tracing::warn!("cannot load scene {path}: {e}");
            None
        }
    }
}

/// Parses a scene file's bytes (text or binary) into a [`PackedScene`].
pub fn packed_scene_from_bytes(bytes: &[u8], path: &str) -> EngineResult<PackedScene> {
    PackedScene::from_tscn(&scene_source_from_bytes(bytes, path)?)
}

/// Converts a loaded binary `PackedScene` resource into `.tscn` source.
pub fn binary_scene_to_tscn(resource: &Resource) -> EngineResult<String> {
    if resource.class_name != "PackedScene" {
        return Err(EngineError::Parse(format!(
            "{} is a {}, not a PackedScene",
            resource.path, resource.class_name
        )));
    }
    let bundle = match resource.get_property("_bundled") {
        Some(Variant::Dictionary(d)) => d.clone(),
        _ => {
            return Err(EngineError::Parse(format!(
                "{}: PackedScene has no _bundled dictionary",
                resource.path
            )))
        }
    };
    let get = |k: &str| bundle.get(k);
    let bad = |what: &str| EngineError::Parse(format!("{}: bundled scene {what}", resource.path));

    let version = match get("version") {
        Some(Variant::Int(v)) => v,
        _ => 1,
    };
    let names: Vec<String> = match get("names") {
        Some(Variant::PackedStringArray(n)) => n,
        _ => return Err(bad("has no names")),
    };
    let variants: Vec<Variant> = match get("variants") {
        Some(Variant::Array(a)) => a.to_vec(),
        _ => Vec::new(),
    };
    let node_count = int_of(get("node_count")).ok_or_else(|| bad("has no node_count"))?;
    let snodes = ints_of(get("nodes")).ok_or_else(|| bad("has no nodes"))?;
    let conn_count = int_of(get("conn_count")).unwrap_or(0);
    let sconns = ints_of(get("conns")).unwrap_or_default();
    let node_paths: Vec<String> = array_of(get("node_paths"))
        .into_iter()
        .map(|v| nodepath_text(&v))
        .collect();
    let editable: Vec<String> = array_of(get("editable_instances"))
        .into_iter()
        .map(|v| nodepath_text(&v))
        .collect();
    let ids = ints_of(get("node_ids")).unwrap_or_default();
    let id_paths: Vec<Vec<i32>> = array_of(get("id_paths"))
        .into_iter()
        .map(|v| match v {
            Variant::PackedInt32Array(p) => p,
            _ => Vec::new(),
        })
        .collect();
    let base_scene = int_of(get("base_scene"));

    // -- decode the node and connection tables (set_bundled_scene) --
    let mut reader = IntReader {
        data: &snodes,
        pos: 0,
    };
    let mut nodes = Vec::with_capacity(node_count.max(0) as usize);
    for _ in 0..node_count {
        let parent = reader.next().ok_or_else(|| bad("node table truncated"))?;
        let owner = reader.next().ok_or_else(|| bad("node table truncated"))?;
        let type_idx = reader.next().ok_or_else(|| bad("node table truncated"))?;
        let name_word = reader.next().ok_or_else(|| bad("node table truncated"))? as u32;
        let instance = reader.next().ok_or_else(|| bad("node table truncated"))?;
        let prop_count = reader.next().ok_or_else(|| bad("node table truncated"))?;
        let mut props = Vec::new();
        for _ in 0..prop_count.max(0) {
            let n = reader.next().ok_or_else(|| bad("node table truncated"))?;
            let v = reader.next().ok_or_else(|| bad("node table truncated"))?;
            props.push((n, v));
        }
        let group_count = reader.next().ok_or_else(|| bad("node table truncated"))?;
        let mut groups = Vec::new();
        for _ in 0..group_count.max(0) {
            groups.push(reader.next().ok_or_else(|| bad("node table truncated"))?);
        }
        nodes.push(NodeData {
            parent,
            owner,
            type_idx,
            name: (name_word & NAME_MASK) as usize,
            // "0 is invalid, stored as 1".
            index: (name_word >> NAME_INDEX_BITS) as i64 - 1,
            instance,
            props,
            groups,
        });
    }

    let mut reader = IntReader {
        data: &sconns,
        pos: 0,
    };
    let mut conns = Vec::new();
    for _ in 0..conn_count {
        let mut next = || {
            reader
                .next()
                .ok_or_else(|| bad("connection table truncated"))
        };
        let from = next()?;
        let to = next()?;
        let signal = next()?;
        let method = next()?;
        let flags = next()?;
        let bind_count = next()?;
        let mut binds = Vec::new();
        for _ in 0..bind_count.max(0) {
            binds.push(next()?);
        }
        let unbinds = if version >= 3 { next()? } else { 0 };
        conns.push(ConnData {
            from,
            to,
            signal,
            method,
            flags,
            binds,
            unbinds,
        });
    }

    let state = State {
        names: &names,
        variants: &variants,
        nodes: &nodes,
        node_paths: &node_paths,
    };
    let name_of = |i: usize| -> EngineResult<&str> {
        names
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| bad("name index out of range"))
    };
    let variant_of = |i: i32| -> EngineResult<&Variant> {
        variants
            .get((i & FLAG_MASK) as usize)
            .ok_or_else(|| bad("variant index out of range"))
    };

    // -- resource tables --
    let mut writer = LiteralWriter::new(resource);

    // Nodes and connections are rendered first so every ExtResource a value
    // reaches (including the deprecated inline-path form) is known before the
    // header sections are written.
    let mut body = String::new();
    for (i, nd) in nodes.iter().enumerate() {
        let mut header = String::from("[node");
        let _ = write!(header, " name=\"{}\"", c_escape(name_of(nd.name)?));
        if nd.type_idx != TYPE_INSTANTIATED {
            let _ = write!(header, " type=\"{}\"", name_of(nd.type_idx as usize)?);
        }
        if let Some(parent) = state.node_path(i, true)? {
            let _ = write!(header, " parent=\"{}\"", c_escape(&parent));
            if !is_unparented(nd.parent) && nd.parent & FLAG_ID_IS_PATH != 0 {
                if let Some(p) = id_paths.get((nd.parent & FLAG_MASK) as usize) {
                    if !p.is_empty() {
                        let _ = write!(header, " parent_id_path={}", int32_array(p));
                    }
                }
            }
        }
        if let Some(owner) = state.owner_path(i)? {
            if owner != "." {
                let _ = write!(header, " owner=\"{}\"", c_escape(&owner));
            }
        }
        if nd.index >= 0 {
            let _ = write!(header, " index=\"{}\"", nd.index);
        }
        if let Some(id) = ids.get(i) {
            // `Node::UNIQUE_SCENE_ID_UNASSIGNED` is 0.
            if *id != 0 {
                let _ = write!(header, " unique_id={id}");
            }
        }
        let deferred: Vec<String> = nd
            .props
            .iter()
            .filter(|(n, _)| n & FLAG_PATH_PROPERTY_IS_NODE != 0)
            .map(|(n, _)| name_of((n & FLAG_PROP_NAME_MASK) as usize).map(|s| s.to_string()))
            .collect::<EngineResult<_>>()?;
        if !deferred.is_empty() {
            let quoted: Vec<String> = deferred
                .iter()
                .map(|s| format!("\"{}\"", c_escape(s)))
                .collect();
            let _ = write!(
                header,
                " node_paths=PackedStringArray({})",
                quoted.join(", ")
            );
        }
        if !nd.groups.is_empty() {
            let mut groups: Vec<&str> = nd
                .groups
                .iter()
                .map(|g| name_of(*g as usize))
                .collect::<EngineResult<_>>()?;
            groups.sort_unstable();
            let quoted: Vec<String> = groups
                .iter()
                .map(|g| format!("\"{}\"", c_escape(g)))
                .collect();
            let _ = write!(header, " groups=[{}]", quoted.join(", "));
        }
        // SceneState::get_node_instance / get_node_instance_placeholder.
        if nd.instance >= 0 {
            let v = variant_of(nd.instance)?;
            if nd.instance & FLAG_INSTANCE_IS_PLACEHOLDER != 0 {
                let _ = write!(header, " instance_placeholder={}", writer.literal(v));
            } else {
                let _ = write!(header, " instance={}", writer.literal(v));
            }
        } else if is_unparented(nd.parent) {
            if let Some(base) = base_scene.filter(|b| *b >= 0) {
                let _ = write!(
                    header,
                    " instance={}",
                    writer.literal(variant_of(base as i32)?)
                );
            }
        }
        header.push(']');
        body.push_str(&header);
        body.push('\n');
        for (n, v) in &nd.props {
            let key = name_of((n & FLAG_PROP_NAME_MASK) as usize)?;
            let value = writer.literal(variant_of(*v)?);
            let _ = writeln!(body, "{} = {}", property_key(key), value);
        }
        body.push('\n');
    }

    for c in &conns {
        let path = |id: i32| -> EngineResult<String> {
            if id & FLAG_ID_IS_PATH != 0 {
                node_paths
                    .get((id & FLAG_MASK) as usize)
                    .cloned()
                    .ok_or_else(|| bad("connection node path out of range"))
            } else {
                Ok(state
                    .node_path((id & FLAG_MASK) as usize, false)?
                    .unwrap_or_else(|| ".".into()))
            }
        };
        let mut line = String::from("[connection");
        let _ = write!(
            line,
            " signal=\"{}\"",
            c_escape(name_of(c.signal as usize)?)
        );
        let _ = write!(line, " from=\"{}\"", c_escape(&path(c.from)?));
        let _ = write!(line, " to=\"{}\"", c_escape(&path(c.to)?));
        let _ = write!(
            line,
            " method=\"{}\"",
            c_escape(name_of(c.method as usize)?)
        );
        if c.flags != CONNECT_PERSIST {
            let _ = write!(line, " flags={}", c.flags);
        }
        if c.unbinds > 0 {
            let _ = write!(line, " unbinds={}", c.unbinds);
        }
        if !c.binds.is_empty() {
            let binds: Vec<String> = c
                .binds
                .iter()
                .map(|b| variant_of(*b).map(|v| writer.literal(v)))
                .collect::<EngineResult<_>>()?;
            let _ = write!(line, " binds=[{}]", binds.join(", "));
        }
        line.push_str("]\n");
        body.push_str(&line);
    }
    if !editable.is_empty() {
        body.push('\n');
        for path in &editable {
            let _ = writeln!(body, "[editable path=\"{}\"]", c_escape(path));
        }
    }

    // Sub-resources are written in index order, which is Godot's save order
    // (a dependency is always saved before its user).
    let mut sub_text = String::new();
    let mut sub_ids: Vec<(usize, &str)> = resource
        .subresources
        .keys()
        .filter_map(|k| k.parse::<usize>().ok().map(|n| (n, k.as_str())))
        .collect();
    sub_ids.sort_unstable();
    for (_, id) in sub_ids {
        let sub = &resource.subresources[id];
        let _ = writeln!(
            sub_text,
            "[sub_resource type=\"{}\" id=\"{id}\"]",
            sub.class_name
        );
        let mut props: Vec<(&String, &Variant)> = sub.properties().collect();
        props.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in props {
            let value = writer.literal(v);
            let _ = writeln!(sub_text, "{} = {}", property_key(k), value);
        }
        sub_text.push('\n');
    }

    let mut out = String::from("[gd_scene format=3");
    if !resource.uid.is_valid() {
        out.push(']');
    } else {
        let _ = write!(
            out,
            " uid=\"{}\"]",
            gdresource::id_to_text(resource.uid.raw())
        );
    }
    out.push_str("\n\n");
    for (id, ext) in &writer.ext {
        let _ = write!(out, "[ext_resource type=\"{}\"", ext.resource_type);
        if !ext.uid.is_empty() {
            let _ = write!(out, " uid=\"{}\"", ext.uid);
        }
        let _ = writeln!(out, " path=\"{}\" id=\"{id}\"]", c_escape(&ext.path));
    }
    if !writer.ext.is_empty() {
        out.push('\n');
    }
    out.push_str(&sub_text);
    out.push_str(&body);
    Ok(out)
}

struct NodeData {
    parent: i32,
    owner: i32,
    type_idx: i32,
    name: usize,
    index: i64,
    instance: i32,
    props: Vec<(i32, i32)>,
    groups: Vec<i32>,
}

struct ConnData {
    from: i32,
    to: i32,
    signal: i32,
    method: i32,
    flags: i32,
    binds: Vec<i32>,
    unbinds: i32,
}

struct IntReader<'a> {
    data: &'a [i32],
    pos: usize,
}

impl IntReader<'_> {
    fn next(&mut self) -> Option<i32> {
        let v = self.data.get(self.pos).copied();
        self.pos += 1;
        v
    }
}

fn is_unparented(parent: i32) -> bool {
    parent < 0 || parent == NO_PARENT_SAVED
}

struct State<'a> {
    names: &'a [String],
    #[allow(dead_code)]
    variants: &'a [Variant],
    nodes: &'a [NodeData],
    node_paths: &'a [String],
}

impl State<'_> {
    /// `SceneState::get_node_path`. `None` is the empty path (the root, asked
    /// for its parent).
    fn node_path(&self, idx: usize, for_parent: bool) -> EngineResult<Option<String>> {
        let err = || EngineError::Parse("bundled scene node index out of range".into());
        let node = self.nodes.get(idx).ok_or_else(err)?;
        if is_unparented(node.parent) {
            return Ok(if for_parent { None } else { Some(".".into()) });
        }
        let mut sub_path: Vec<String> = Vec::new();
        let mut base_path: Option<&str> = None;
        let mut nidx = idx;
        // A well-formed scene's parent chain is at most as long as its node
        // list; anything longer is a cycle in corrupt data.
        for _ in 0..=self.nodes.len() {
            let n = self.nodes.get(nidx).ok_or_else(err)?;
            if is_unparented(n.parent) {
                sub_path.insert(0, ".".into());
                break;
            }
            if !for_parent || idx != nidx {
                sub_path.insert(0, self.names.get(n.name).cloned().ok_or_else(err)?);
            }
            if n.parent & FLAG_ID_IS_PATH != 0 {
                base_path = Some(
                    self.node_paths
                        .get((n.parent & FLAG_MASK) as usize)
                        .map(String::as_str)
                        .ok_or_else(err)?,
                );
                break;
            }
            nidx = (n.parent & FLAG_MASK) as usize;
        }
        let mut parts: Vec<String> = Vec::new();
        if let Some(base) = base_path {
            parts.extend(
                base.split('/')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            );
        }
        parts.extend(sub_path);
        // NodePath::simplified: drop "." segments, but a path of only "." is ".".
        let simplified: Vec<&str> = parts
            .iter()
            .map(String::as_str)
            .filter(|s| *s != ".")
            .collect();
        if simplified.is_empty() {
            return Ok(Some(".".into()));
        }
        Ok(Some(simplified.join("/")))
    }

    /// `SceneState::get_node_owner_path`; `None` for no owner.
    fn owner_path(&self, idx: usize) -> EngineResult<Option<String>> {
        let owner = self.nodes[idx].owner;
        if owner < 0 || owner == NO_PARENT_SAVED {
            return Ok(None);
        }
        if owner & FLAG_ID_IS_PATH != 0 {
            return Ok(self.node_paths.get((owner & FLAG_MASK) as usize).cloned());
        }
        self.node_path((owner & FLAG_MASK) as usize, false)
    }
}

/// Renders values as `.tscn` literals, turning the binary loader's
/// `"ExtResource:<idx>"` / `"SubResource:<idx>"` reference strings back into
/// `ExtResource("<idx>")` / `SubResource("<idx>")` calls and collecting the
/// external resources they reach.
struct LiteralWriter {
    known: HashMap<String, gdresource::ExtResource>,
    /// The ext_resource sections to write, by id.
    ext: BTreeMap<String, gdresource::ExtResource>,
}

impl LiteralWriter {
    fn new(resource: &Resource) -> Self {
        Self {
            known: resource.ext_resources.clone(),
            ext: resource
                .ext_resources
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    fn literal(&mut self, v: &Variant) -> String {
        match v {
            Variant::String(s) => {
                if let Some(id) = s.strip_prefix("ExtResource:") {
                    if !self.known.contains_key(id) {
                        // The deprecated inline form carries the PATH, not an
                        // index: give it a table entry of its own.
                        let new_id = format!("inline_{}", self.ext.len());
                        let entry = gdresource::ExtResource {
                            resource_type: "Resource".into(),
                            uid: String::new(),
                            path: id.to_string(),
                            id: new_id.clone(),
                        };
                        self.known.insert(id.to_string(), entry.clone());
                        self.ext.insert(new_id.clone(), entry);
                        return format!("ExtResource(\"{new_id}\")");
                    }
                    let real = self.known[id].id.clone();
                    return format!("ExtResource(\"{real}\")");
                }
                if let Some(id) = s.strip_prefix("SubResource:") {
                    return format!("SubResource(\"{id}\")");
                }
                crate::scene_saver::format_variant_value(v)
            }
            Variant::Array(a) => {
                let items: Vec<String> = a.to_vec().iter().map(|i| self.literal(i)).collect();
                format!("[{}]", items.join(", "))
            }
            Variant::Dictionary(d) => {
                let pairs: Vec<String> = d
                    .to_dict()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", self.literal(k), self.literal(v)))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            }
            Variant::Float(f) if f.is_nan() => "nan".into(),
            Variant::Float(f) if f.is_infinite() => {
                if *f > 0.0 {
                    "inf".into()
                } else {
                    "-inf".into()
                }
            }
            other => crate::scene_saver::format_variant_value(other),
        }
    }
}

/// A property key as the text format writes it: bare when it is a plain
/// identifier path (`metadata/_editor_floor_`), quoted otherwise.
fn property_key(key: &str) -> String {
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | ':' | '.' | '-'));
    if plain {
        key.to_string()
    } else {
        format!("\"{}\"", c_escape(key))
    }
}

fn c_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn int32_array(v: &[i32]) -> String {
    let items: Vec<String> = v.iter().map(i32::to_string).collect();
    format!("PackedInt32Array({})", items.join(", "))
}

fn int_of(v: Option<Variant>) -> Option<i32> {
    match v? {
        Variant::Int(i) => i32::try_from(i).ok(),
        _ => None,
    }
}

fn ints_of(v: Option<Variant>) -> Option<Vec<i32>> {
    match v? {
        Variant::PackedInt32Array(a) => Some(a),
        _ => None,
    }
}

fn array_of(v: Option<Variant>) -> Vec<Variant> {
    match v {
        Some(Variant::Array(a)) => a.to_vec(),
        _ => Vec::new(),
    }
}

fn nodepath_text(v: &Variant) -> String {
    match v {
        Variant::NodePath(p) => p.to_string(),
        Variant::String(s) => s.clone(),
        other => other.to_string_lossy(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platformer_3d(rel: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../games/platformer_3d")
            .join(rel);
        std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    }

    /// The real level converts to text that names the GridMap, its MeshLibrary
    /// and its cell data, and that the `.tscn` parser accepts.
    #[test]
    fn real_grid_map_scn_converts_to_tscn() {
        let bytes = platformer_3d("stage/grid_map.scn");
        let text = scene_source_from_bytes(&bytes, "res://stage/grid_map.scn").unwrap();
        assert!(
            text.starts_with("[gd_scene format=3 uid=\"uid://"),
            "{}",
            &text[..80]
        );
        assert!(
            text.contains("[ext_resource type=\"MeshLibrary\" uid=\"uid://b4f5b2j76nf2b\" path=\"res://stage/tiles.tres\" id=\"0\"]"),
            "{}",
            &text[..400.min(text.len())]
        );
        assert!(text.contains("[node name=\"GridMap\" type=\"GridMap\""));
        assert!(text.contains("mesh_library = ExtResource(\"0\")"));
        assert!(text.contains("data = {\"cells\": PackedInt32Array("));

        let scene = PackedScene::from_tscn(&text).unwrap();
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].class_name(), "GridMap");
        assert_eq!(nodes[0].name(), "GridMap");
    }

    #[test]
    fn text_scenes_pass_through_and_garbage_is_an_error() {
        let src = "[gd_scene format=3]\n\n[node name=\"A\" type=\"Node3D\"]\n";
        assert_eq!(
            scene_source_from_bytes(src.as_bytes(), "res://a.tscn").unwrap(),
            src
        );
        assert!(scene_source_from_bytes(&[0xff, 0xfe, 0], "res://a.tscn").is_err());
        // A binary resource that is not a scene is refused, not mis-instanced.
        let mesh = platformer_3d("stage/meshes/floor.res");
        let err = scene_source_from_bytes(&mesh, "res://stage/meshes/floor.res").unwrap_err();
        assert!(err.to_string().contains("not a PackedScene"), "{err}");
    }

    /// Hand-built bundle exercising the parts the real level does not use:
    /// a child hierarchy, groups, an index, a NodePath property and a
    /// connection with binds.
    #[test]
    fn bundle_with_children_groups_and_connections_round_trips() {
        let names = vec![
            "Root".to_string(),      // 0
            "Node3D".to_string(),    // 1
            "Child".to_string(),     // 2
            "Grand".to_string(),     // 3
            "target".to_string(),    // 4
            "enemies".to_string(),   // 5
            "pressed".to_string(),   // 6
            "_on_press".to_string(), // 7
            "Button".to_string(),    // 8
        ];
        let variants = gdvariant::SharedArray::from_vec(vec![
            Variant::NodePath(gdcore::node_path::NodePath::new("../Child")),
            Variant::Int(7),
        ]);
        #[rustfmt::skip]
        let nodes = vec![
            // root: parent none, owner none, type Node3D, name Root, no instance, 0 props, 0 groups
            NO_PARENT_SAVED, NO_PARENT_SAVED, 1, 0, -1, 0, 0,
            // Child of root (node 0), owner root, type Button, index 0 stored as 1
            0, 0, 8, 2 | (1 << NAME_INDEX_BITS), -1, 0, 1, 5,
            // Grand under Child, property `target` (a node path) = variants[0]
            1, 0, 1, 3, -1, 1, 4 | FLAG_PATH_PROPERTY_IS_NODE, 0, 0,
        ];
        #[rustfmt::skip]
        let conns = vec![
            // from Child (1) to root (0), signal pressed, method _on_press, flags 2|1, 1 bind, unbinds 0
            1, 0, 6, 7, 3, 1, 1, 0,
        ];
        let bundle = gdvariant::SharedDictionary::new();
        bundle
            .insert(
                Variant::String("names".into()),
                Variant::PackedStringArray(names),
            )
            .unwrap();
        bundle
            .insert(Variant::String("variants".into()), Variant::Array(variants))
            .unwrap();
        bundle
            .insert(Variant::String("node_count".into()), Variant::Int(3))
            .unwrap();
        bundle
            .insert(
                Variant::String("nodes".into()),
                Variant::PackedInt32Array(nodes),
            )
            .unwrap();
        bundle
            .insert(Variant::String("conn_count".into()), Variant::Int(1))
            .unwrap();
        bundle
            .insert(
                Variant::String("conns".into()),
                Variant::PackedInt32Array(conns),
            )
            .unwrap();
        bundle
            .insert(Variant::String("version".into()), Variant::Int(3))
            .unwrap();
        let mut res = Resource::new("PackedScene");
        res.set_property("_bundled", Variant::Dictionary(bundle));

        let text = binary_scene_to_tscn(&res).unwrap();
        assert!(
            text.contains("[node name=\"Root\" type=\"Node3D\"]"),
            "{text}"
        );
        assert!(
            text.contains("[node name=\"Child\" type=\"Button\" parent=\".\" index=\"0\" groups=[\"enemies\"]]"),
            "{text}"
        );
        assert!(
            text.contains("[node name=\"Grand\" type=\"Node3D\" parent=\"Child\" node_paths=PackedStringArray(\"target\")]"),
            "{text}"
        );
        assert!(text.contains("target = NodePath(\"../Child\")"), "{text}");
        assert!(
            text.contains("[connection signal=\"pressed\" from=\"Child\" to=\".\" method=\"_on_press\" flags=3 binds=[7]]"),
            "{text}"
        );

        let scene = PackedScene::from_tscn(&text).unwrap();
        assert_eq!(scene.node_count(), 3);
        assert_eq!(scene.connection_count(), 1);
        let nodes = scene.instance().unwrap();
        assert_eq!(nodes[2].name(), "Grand");
        assert_eq!(nodes[1].groups().len(), 1);
    }

    #[test]
    fn a_truncated_node_table_is_an_error_not_a_panic() {
        let bundle = gdvariant::SharedDictionary::new();
        bundle
            .insert(
                Variant::String("names".into()),
                Variant::PackedStringArray(vec!["A".into()]),
            )
            .unwrap();
        bundle
            .insert(Variant::String("node_count".into()), Variant::Int(2))
            .unwrap();
        bundle
            .insert(
                Variant::String("nodes".into()),
                Variant::PackedInt32Array(vec![NO_PARENT_SAVED, NO_PARENT_SAVED, 0, 0]),
            )
            .unwrap();
        let mut res = Resource::new("PackedScene");
        res.set_property("_bundled", Variant::Dictionary(bundle));
        assert!(binary_scene_to_tscn(&res).is_err());
    }
}
