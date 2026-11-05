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
use crate::tilemap::TILE_SET_PREFIX;

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
    ///
    /// Godot's `Object.ConnectFlags` bitfield as written by the text format:
    /// `CONNECT_DEFERRED` 1, `CONNECT_PERSIST` 2, `CONNECT_ONE_SHOT` 4,
    /// `CONNECT_REFERENCE_COUNTED` 8.
    pub flags: u32,
    /// Extra bound arguments delivered to the callback after the signal's own
    /// arguments (Godot `binds=[...]` on the connection). Empty when absent.
    pub binds: Vec<Variant>,
    /// How many trailing signal arguments the callback drops (Godot
    /// `unbinds=N`). Zero when absent.
    pub unbinds: usize,
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
    /// `res://` path from `instance_placeholder="..."` — the sub-scene is
    /// deferred (a placeholder stands in) rather than instanced on load.
    instance_placeholder: Option<String>,
    /// Whether the header carried `type=`. Godot omits it exactly when the
    /// node's class is defined in ANOTHER file: an `instance=` node, or an
    /// override entry for a node inside an instanced/inherited scene. Such a
    /// node must not be seeded with `Node`'s ClassDB defaults, because when the
    /// sub-scene is expanded onto it those seeded values would be
    /// indistinguishable from authored overrides and would beat the
    /// sub-scene's own values.
    has_type: bool,
}

impl NodeTemplate {
    /// Closes a template once its `[node]` section has been read.
    ///
    /// 🔑 Godot 4 saves "Access as Unique Name" as the PROPERTY
    /// `unique_name_in_owner = true` (Node's `set_unique_name_in_owner`), not
    /// as a `%` in the header's `name=` — which is the only spelling this
    /// loader used to recognise. Every editor-saved scene therefore lost the
    /// flag, and `%Name` lookups failed with "node not found" (the official 3D
    /// platformer's `%CoinCount`, pat-v3co1). Folded into `unique_name` here,
    /// once, so every instancing path that copies `tmpl.unique_name` agrees;
    /// the property row stays so a save round-trips it.
    fn finish(mut self) -> Self {
        if matches!(
            self.properties.get("unique_name_in_owner"),
            Some(Variant::Bool(true))
        ) {
            self.unique_name = true;
        }
        self
    }
}

/// An external resource reference parsed from `[ext_resource]` sections.
#[derive(Debug, Clone)]
pub struct ExtResourceEntry {
    /// The resource type (e.g. `"Script"`, `"PackedScene"`, `"Texture2D"`).
    pub res_type: String,
    /// The resource path (e.g. `"res://scripts/player.gd"`).
    pub path: String,
    /// The `uid://...` the section declared, if any (pat-0q2kx.137).
    ///
    /// Godot writes `uid=` on every `[ext_resource]` it saves and resolves a
    /// moved file through it. Dropping it here meant an editor open+save
    /// rewrote the scene without uids, so the references degraded to
    /// path-only and stopped surviving a move.
    pub uid: Option<String>,
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
    /// Inline sub-resource types: id -> class name (e.g. `"Environment"`).
    sub_resources: HashMap<String, String>,
    /// Inline sub-resource properties: id -> parsed key/value pairs. Lets
    /// callers resolve a node's `prop = SubResource("id")` to the concrete
    /// resource's fields (e.g. a `WorldEnvironment`'s `Environment`).
    sub_resource_props: HashMap<String, HashMap<String, Variant>>,
    /// Node paths from `[editable path="..."]` sections — sub-scene instances
    /// whose local property/child overrides should be applied on load.
    editable_instances: Vec<String>,
}

/// Percent-encodes a marker property value.
///
/// String properties (a `TextMesh`'s `text`, a font path) travel in the
/// `primitive:<Class>?k=v` marker, whose separators are `?`, `&` and `=`. Any
/// of those inside a value would split the marker at the wrong place, and a
/// space would break a path.
///
/// ⚠️ Deliberately a **single pass** over the bytes. Escaping with chained
/// `.replace()` calls double-escapes: replacing `%` after `&` turns an already
/// emitted `%26` into `%2526`. One pass cannot make that mistake.
pub fn percent_encode_marker_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let c = *byte;
        // RFC 3986 unreserved set; everything else is escaped, which is more
        // than strictly needed and cheaper to be sure of.
        if c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(c as char);
        } else {
            out.push('%');
            out.push(char::from_digit((c >> 4) as u32, 16).unwrap_or('0'));
            out.push(char::from_digit((c & 0x0f) as u32, 16).unwrap_or('0'));
        }
    }
    out
}

/// Reverses [`percent_encode_marker_value`].
///
/// A malformed escape is passed through literally rather than dropped: a marker
/// that was never encoded (or was hand-written) still reads back as itself.
pub fn percent_decode_marker_value(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Invalid UTF-8 can only come from a hand-edited marker; keep the lossy
    // reading rather than losing the whole value.
    String::from_utf8_lossy(&out).into_owned()
}

/// Encodes a primitive mesh sub-resource's numeric properties into the
/// `?key=value` suffix of a `primitive:<Class>` marker.
///
/// The editor viewport resolves a mesh from the marker string alone and
/// **caches by that string**, so the properties have to travel in it: two tori
/// differing only in radius would otherwise share a cache key and the second
/// would draw as the first. `split_mesh_marker` already parses this form.
///
/// Vectors flatten component-wise (`size.x`) because that is how the shared
/// resolver addresses them, and this stays free of per-mesh-type knowledge:
/// anything numeric comes along, anything else is skipped.
///
/// ⚠️ Keys are sorted. Property maps have no stable iteration order, so an
/// unsorted marker would differ run to run — thrashing the editor's cache and
/// making saved scenes and test expectations non-deterministic.
fn primitive_marker_query(props: Option<&HashMap<String, Variant>>) -> String {
    let Some(props) = props else {
        return String::new();
    };

    let mut pairs: Vec<(String, f32)> = Vec::new();
    let mut strings: Vec<(String, String)> = Vec::new();
    for (key, value) in props {
        match value {
            Variant::Float(f) => pairs.push((key.clone(), *f as f32)),
            Variant::Int(i) => pairs.push((key.clone(), *i as f32)),
            Variant::Bool(b) => pairs.push((key.clone(), if *b { 1.0 } else { 0.0 })),
            Variant::Vector2(v) => {
                pairs.push((format!("{key}.x"), v.x));
                pairs.push((format!("{key}.y"), v.y));
            }
            Variant::Vector3(v) => {
                pairs.push((format!("{key}.x"), v.x));
                pairs.push((format!("{key}.y"), v.y));
                pairs.push((format!("{key}.z"), v.z));
            }
            // A TextMesh's `text` and `font` are the only string-valued
            // primitive properties, and the editor resolves from the marker
            // alone, so they have to travel in it too (pat-s3rrv.199).
            Variant::String(text) => {
                strings.push((key.clone(), percent_encode_marker_value(text)));
            }
            _ => {}
        }
    }
    if pairs.is_empty() && strings.is_empty() {
        return String::new();
    }
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    strings.sort_by(|a, b| a.0.cmp(&b.0));

    let mut body: Vec<String> = pairs.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
    body.extend(strings.into_iter().map(|(k, v)| format!("{k}={v}")));
    format!("?{}", body.join("&"))
}

/// The value an instanced node receives for a property stored in the scene.
///
/// Arrays and dictionaries are handles to shared storage (pat-s3rrv.8), and a
/// `PackedScene` is parsed once and instanced many times, so handing every
/// instance the parsed handle would make two instances of one scene share
/// their `Array`/`Dictionary` properties. Godot's `SceneState::instantiate`
/// gives each instance its own shallow copy (`set_array.duplicate()` /
/// `set_dict.duplicate()`, scene/resources/packed_scene.cpp), so do the same.
fn instance_value(value: &Variant) -> Variant {
    value.duplicate(false)
}

impl PackedScene {
    /// Parses a `.tscn` string into a `PackedScene`.
    pub fn from_tscn(source: &str) -> EngineResult<Self> {
        let mut uid = None;
        let mut templates: Vec<NodeTemplate> = Vec::new();
        let mut connections: Vec<SceneConnection> = Vec::new();
        let mut editable_instances: Vec<String> = Vec::new();
        let mut ext_resources: HashMap<String, ExtResourceEntry> = HashMap::new();
        // Inline sub-resources: id -> type (e.g. "SphereMesh"). Used to resolve a
        // node's `mesh = SubResource("id")` to a concrete primitive type so the
        // 3D viewport can draw the real shape.
        let mut sub_resources: HashMap<String, String> = HashMap::new();
        // Inline sub-resource property lines (id -> {key: value}), so e.g. a
        // CollisionShape3D's BoxShape3D `size` can size its wireframe gizmo.
        let mut sub_resource_props: HashMap<String, HashMap<String, Variant>> = HashMap::new();
        let mut current: Option<NodeTemplate> = None;
        // The id of the sub-resource whose property lines we're currently reading.
        let mut current_sub: Option<String> = None;
        // A property whose value opens a bracket/brace that has not yet closed,
        // so it continues onto following lines. Godot writes container values
        // (e.g. a GridMap `data = { "cells": PackedInt32Array(...) }`) across
        // multiple lines; we accumulate until the brackets balance, then parse.
        let mut pending_prop: Option<(String, String)> = None;

        for (line_num, raw_line) in source.lines().enumerate() {
            let _line_num = line_num + 1; // 1-based, for error reporting
            let line = raw_line.trim();

            // Continue accumulating a multi-line property value from a prior line.
            if let Some((key, mut acc)) = pending_prop.take() {
                if gdresource::loader::ends_inside_string(&acc) {
                    // A newline INSIDE a string literal (`text = "Dodge the`
                    // / `Creeps"`): the line is the string's own text, so it is
                    // joined by the newline it contains and kept verbatim.
                    acc.push('\n');
                    acc.push_str(raw_line);
                } else {
                    if !acc.is_empty() {
                        acc.push(' ');
                    }
                    acc.push_str(line);
                }
                if gdresource::loader::value_continues(&acc) {
                    // Still unbalanced — keep reading subsequent lines.
                    pending_prop = Some((key, acc));
                } else if let Some(ref mut tmpl) = current {
                    match parse_variant_value(acc.trim()) {
                        Ok(value) => {
                            tmpl.properties.insert(key, value);
                        }
                        Err(_) => {
                            tracing::warn!("skipping unparseable multi-line value for key '{key}'");
                        }
                    }
                } else if let Some(ref sub_id) = current_sub {
                    // `[sub_resource]` bodies span lines too: Godot writes an
                    // Animation's `tracks/N/keys` dictionary and an
                    // AnimationLibrary's `_data` map across several lines
                    // (pat-0q2kx.1). Without this branch the whole keyframe
                    // table was silently dropped.
                    match parse_variant_value(acc.trim()) {
                        Ok(value) => {
                            sub_resource_props
                                .entry(sub_id.clone())
                                .or_default()
                                .insert(key, value);
                        }
                        Err(_) => {
                            tracing::warn!(
                                "skipping unparseable multi-line sub-resource value for key '{key}'"
                            );
                        }
                    }
                }
                continue;
            }

            // Skip empty / comments.
            if line.is_empty() || line.starts_with(';') {
                continue;
            }

            // Section header.
            if line.starts_with('[') && line.ends_with(']') {
                // A new section ends any sub-resource we were reading.
                current_sub = None;
                // Flush previous node template.
                if let Some(tmpl) = current.take() {
                    templates.push(tmpl.finish());
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
                            // `binds=[...]` — extra args prepended to the callback.
                            let binds = attrs
                                .get("binds")
                                .and_then(|s| parse_variant_value(s).ok())
                                .and_then(|v| match v {
                                    Variant::Array(a) => Some(a),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            // `unbinds=N` — trailing signal args the callback
                            // drops. Written by Godot's text format next to
                            // `binds=`, and previously not parsed at all, so a
                            // scene authored with it silently delivered the
                            // extra arguments (pat-0q2kx.65).
                            let unbinds = attrs
                                .get("unbinds")
                                .and_then(|v| v.parse::<usize>().ok())
                                .unwrap_or(0);
                            connections.push(SceneConnection {
                                signal_name: signal,
                                from_path: from,
                                to_path: to,
                                method_name: method,
                                flags,
                                binds: binds.to_vec(),
                                unbinds,
                            });
                        }
                        _ => {
                            tracing::warn!(
                                "skipping malformed [connection] — missing required attributes"
                            );
                        }
                    }
                } else if inner.starts_with("editable") {
                    // `[editable path="Sub/Instance"]` — mark a sub-scene instance
                    // as having local overrides that apply on load.
                    let attrs = extract_header_attrs(inner);
                    if let Some(path) = attrs.get("path").cloned() {
                        editable_instances.push(path);
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

                    let has_type = attrs.contains_key("type");
                    let class_name = attrs.get("type").cloned().unwrap_or_else(|| "Node".into());
                    let parent_path = attrs.get("parent").cloned();

                    // Parse groups=["group1", "group2"] attribute.
                    let groups = attrs
                        .get("groups")
                        .map(|g| parse_groups_attr(g))
                        .unwrap_or_default();

                    // Parse instance=ExtResource("id") attribute.
                    let instance = attrs.get("instance").cloned();

                    // Parse instance_placeholder="res://..." (deferred instancing).
                    let instance_placeholder = attrs.get("instance_placeholder").cloned();

                    current = Some(NodeTemplate {
                        name,
                        class_name,
                        parent_path,
                        properties: HashMap::new(),
                        groups,
                        unique_name,
                        instance,
                        script_path: None,
                        instance_placeholder,
                        has_type,
                    });
                } else if inner.starts_with("ext_resource") {
                    let attrs = extract_header_attrs(inner);
                    if let (Some(id), Some(path)) =
                        (attrs.get("id").cloned(), attrs.get("path").cloned())
                    {
                        let res_type = attrs.get("type").cloned().unwrap_or_default();
                        let uid = attrs.get("uid").cloned();
                        ext_resources.insert(
                            id,
                            ExtResourceEntry {
                                res_type,
                                path,
                                uid,
                            },
                        );
                    }
                } else if inner.starts_with("sub_resource") {
                    // Capture the type by id so `mesh = SubResource("id")` can be
                    // resolved to a primitive. The sub-resource's own property
                    // lines that follow have no `current` node and are skipped.
                    let attrs = extract_header_attrs(inner);
                    if let (Some(id), Some(ty)) =
                        (attrs.get("id").cloned(), attrs.get("type").cloned())
                    {
                        sub_resources.insert(id.clone(), ty);
                        sub_resource_props.entry(id.clone()).or_default();
                        current_sub = Some(id);
                    }
                }
                // Ignore other sections for this simplified parser.
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
                            // Godot applies a node's stored properties IN FILE
                            // ORDER (packed_scene.cpp `SceneState::instantiate`):
                            // a script variable authored ABOVE `script = …` is set
                            // on the script-less node, is not a property of its
                            // class, and is dropped — the variable keeps its
                            // initializer. Record those keys so the script
                            // replay (`SceneTree::attach_script`) skips them.
                            // Godot's own saver always writes `script` first, so
                            // this only fires for hand-ordered files.
                            let before: Vec<Variant> = tmpl
                                .properties
                                .keys()
                                .filter(|k| !k.starts_with('_'))
                                .filter(|k| {
                                    !gdobject::class_db::class_has_property(&tmpl.class_name, k)
                                })
                                .map(|k| Variant::String(k.clone()))
                                .collect();
                            if !before.is_empty() {
                                tmpl.properties.insert(
                                    PROPS_BEFORE_SCRIPT_KEY.to_string(),
                                    Variant::Array(before.into()),
                                );
                            }
                        }
                        // Store the raw reference as a string property too.
                        tmpl.properties
                            .insert(key.to_string(), Variant::String(value_str.to_string()));
                        continue;
                    }

                    // A value that opens more brackets/braces than it closes on
                    // this line continues onto following lines (e.g. a GridMap
                    // `data = {` dictionary). Buffer it until the brackets balance.
                    if gdresource::loader::value_continues(value_str) {
                        pending_prop = Some((key.to_string(), value_str.to_string()));
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
            } else if let Some(ref sub_id) = current_sub {
                // Property lines inside a [sub_resource] section (e.g. a
                // BoxShape3D's `size`), kept so the gizmo can use real extents.
                if let Some((key, value_str)) = line.split_once('=') {
                    let key = key.trim();
                    let value_str = value_str.trim();
                    // Container values continue onto following lines, exactly as
                    // for node properties above (Animation `tracks/N/keys`,
                    // AnimationLibrary `_data`).
                    if gdresource::loader::value_continues(value_str) {
                        pending_prop = Some((key.to_string(), value_str.to_string()));
                        continue;
                    }
                    // The three other parse sites warn when they drop a value;
                    // this one discarded silently, so a sub-resource property the
                    // parser cannot read yet (an int vector, `Vector4`,
                    // `Projection`) vanished with no trace at all in the logs
                    // (pat-nzgdu.78).
                    match parse_variant_value(value_str) {
                        Ok(value) => {
                            sub_resource_props
                                .entry(sub_id.clone())
                                .or_default()
                                .insert(key.to_string(), value);
                        }
                        Err(_) => {
                            tracing::warn!(
                                "skipping unparseable sub-resource value for key '{key}': {value_str}"
                            );
                        }
                    }
                }
            }
        }

        // Flush last template.
        if let Some(tmpl) = current.take() {
            templates.push(tmpl.finish());
        }

        // pat-ghfv0.40: record inline SubResource references for the served
        // inspector BEFORE the mesh/shape resolution below overwrites the raw
        // `SubResource:<id>` values with viewport markers. For every property that
        // is an inline SubResource ref, remember `resource_class::sub_id` under a
        // `__resref__<prop>` companion key so `/api/node` can present it as Godot
        // does (`{resource_class, resource_path = "<scene res://>::<id>"}`). The
        // companion is hidden from the inspector's property list and skipped by
        // the scene saver.
        for tmpl in &mut templates {
            let refs: Vec<(String, String)> = tmpl
                .properties
                .iter()
                .filter_map(|(k, v)| match v {
                    Variant::String(s) => {
                        let id = s.strip_prefix("SubResource:")?;
                        let class = sub_resources.get(id).cloned().unwrap_or_default();
                        Some((k.clone(), format!("{class}::{id}")))
                    }
                    _ => None,
                })
                .collect();
            for (prop, meta) in refs {
                // The sub-resource's body, copied VERBATIM under
                // `__subres__<prop>__<key>` so a save can write it back
                // (pat-s3rrv.207).
                //
                // 🔑 This is what makes an unknown resource survive a round
                // trip. `__resref__` already records `class::id`, so the saver
                // writes the right `[sub_resource type=...]` header for any
                // class — but the BODY was recovered through
                // `companion_prefix_for`, a four-entry allowlist keyed on the
                // PROPERTY name. Anything outside it (a plugin's custom
                // Resource, but equally a MultiMesh or a Skin) got a section
                // header with no properties, so the data was dropped silently
                // on save. Godot's answer is MissingResource: record everything
                // verbatim and hand it back untouched.
                //
                // ⚠️ Verbatim, and deliberately SEPARATE from the specialised
                // companions beside it. Those hold values the engine has
                // resolved for its own use — `__canvas_texture__diffuse_texture`
                // is a filesystem path where the file said `ExtResource("1_d")`
                // — so writing them back would silently rewrite the author's
                // scene. This copy is never read by a render path.
                if let Some(sub_id) = meta.rsplit_once("::").map(|(_, id)| id) {
                    if let Some(body) = sub_resource_props.get(sub_id) {
                        let copied: Vec<(String, Variant)> = body
                            .iter()
                            .map(|(k, v)| (format!("__subres__{prop}__{k}"), v.clone()))
                            .collect();
                        for (key, value) in copied {
                            tmpl.properties.insert(key, value);
                        }
                    }
                }

                // A scripted resource's script path, carried to the node as
                // `__resscript__<prop>` (pat-s3rrv.160).
                //
                // Render time has the tree but not the PackedScene, so without
                // this a scripted StyleBox or Texture2D is unreachable once the
                // scene is loaded: `__resref__` names the class and sub-id, and
                // the id resolves only against sub-resource bodies that live in
                // the scene object. The node already carries `_script_path` for
                // its OWN script; this is the same idea for the resources it
                // holds.
                if let Some(sub_id) = meta.rsplit_once("::").map(|(_, id)| id) {
                    let script_path = sub_resource_props
                        .get(sub_id)
                        .and_then(|props| props.get("script"))
                        .and_then(|v| match v {
                            Variant::String(s) => s.strip_prefix("ExtResource:"),
                            _ => None,
                        })
                        .and_then(|ext_id| ext_resources.get(ext_id))
                        .map(|entry| entry.path.clone());
                    if let Some(path) = script_path {
                        tmpl.properties
                            .insert(format!("__resscript__{prop}"), Variant::String(path));
                    }
                }
                // A `shortcut` pointing at a Shortcut resource is flattened to
                // the accelerators it declares (pat-s3rrv.175), because at input
                // time the tree is all we have — the Shortcut's `events` array
                // lives in the scene's sub-resource bodies and is unreachable
                // once loading is done.
                //
                // Stored as [keycode, modifier_mask, keycode, modifier_mask, …]
                // in ONE array rather than two parallel ones: parallel arrays
                // read by index are coupled through their length, and a filter
                // applied to one silently misaligns the other.
                if prop == "shortcut" {
                    if let Some(sub_id) = meta.rsplit_once("::").map(|(_, id)| id) {
                        let pairs = shortcut_event_pairs(&sub_resource_props, sub_id);
                        if !pairs.is_empty() {
                            tmpl.properties.insert(
                                "__shortcut__events".to_string(),
                                Variant::PackedInt32Array(pairs),
                            );
                        }
                        let actions = shortcut_actions(&sub_resource_props, sub_id);
                        if !actions.is_empty() {
                            tmpl.properties.insert(
                                crate::shortcut::SHORTCUT_ACTIONS_KEY.to_string(),
                                Variant::PackedStringArray(actions),
                            );
                        }
                    }
                }
                tmpl.properties
                    .insert(format!("__resref__{prop}"), Variant::String(meta));
            }
        }

        // Per-surface material overrides BEYOND surface 0 (pat-s3rrv.187).
        //
        // The `__material__*` pass below flattens exactly one material — Godot's
        // mesh-level priority picks `material_override`, then
        // `surface_material_override/0`, then the mesh's own `material` — so a
        // two-material mesh lost its second material silently at load, and with
        // it any chance of writing it back out.
        //
        // ⚠️ A DISTINCT prefix, not a suffix inside `__material__`. The surface-0
        // collector rebuilds its body with `strip_prefix("__material__")`, so a
        // key spelled `__material__1__albedo_color` would be harvested into
        // surface 0's sub-resource as a property literally named
        // `1__albedo_color` — corrupting the material that already worked.
        //
        // The `__resref__surface_material_override/N` companion these pair with
        // is already written by the generic sub-resource pass above, for every N.
        for tmpl in &mut templates {
            let indexed: Vec<(usize, String)> = tmpl
                .properties
                .iter()
                .filter_map(|(k, v)| {
                    let idx: usize = k.strip_prefix("surface_material_override/")?.parse().ok()?;
                    // Surface 0 is the `__material__*` pass's job; duplicating it
                    // here would write the same bag under two prefixes.
                    if idx == 0 {
                        return None;
                    }
                    match v {
                        Variant::String(sub) => {
                            Some((idx, sub.strip_prefix("SubResource:")?.to_string()))
                        }
                        _ => None,
                    }
                })
                .collect();
            for (idx, mat_id) in indexed {
                let Some(props) = sub_resource_props.get(&mat_id) else {
                    continue;
                };
                let flattened: Vec<(String, Variant)> = props
                    .iter()
                    .map(|(k, v)| (format!("__surfmat{idx}__{k}"), v.clone()))
                    .collect();
                for (k, v) in flattened {
                    tmpl.properties.insert(k, v);
                }
            }
        }

        // Flatten a `MultiplayerSynchronizer`'s `SceneReplicationConfig`
        // sub-resource onto the node under `__replication__*` (pat-s3rrv.17),
        // mirroring the `__material__*` pass below.
        //
        // ⚠️ This is what a REAL Godot-exported scene looks like. The editor
        // writes `replication_config = SubResource("...")` with
        // `properties/N/path` entries inside it; Patina only ever read a
        // hand-authored `_sync_properties` array, so every synchronizer exported
        // from the actual editor replicated NOTHING — and did so silently, since
        // an empty property list is also how "not configured yet" is spelled.
        //
        // Copied verbatim, like the material pass: by render/tick time the
        // `PackedScene` is gone and only the `SceneTree` remains, so anything
        // the tick needs has to travel on the node.
        for tmpl in &mut templates {
            let Some(cfg_id) = tmpl
                .properties
                .get("replication_config")
                .and_then(|v| match v {
                    Variant::String(s) => s.strip_prefix("SubResource:").map(String::from),
                    _ => None,
                })
            else {
                continue;
            };
            if let Some(cfg_props) = sub_resource_props.get(&cfg_id) {
                for (key, value) in cfg_props {
                    tmpl.properties
                        .insert(format!("__replication__{key}"), value.clone());
                }
            }
        }

        // Flatten a `BaseButton`'s `ButtonGroup` sub-resource onto the node
        // under `__button_group__*` (pat-s3rrv.161), mirroring the
        // `__replication__*` pass above.
        //
        // ⚠️ Without this, `allow_unpress` is unreachable at runtime. The group
        // reference survives on the node, so exclusivity worked, but the flag
        // that says whether the active member may be clicked OFF lives in the
        // body — and the body is dropped once the `PackedScene` is gone. The
        // symptom was silent: a group with `allow_unpress = true` behaved
        // exactly like one without it.
        //
        // ⚠️ Only INLINE groups are covered. A `.tres` `ButtonGroup` shared
        // between scenes arrives as `ExtResource`, whose body is in another
        // file this pass never parses; those groups keep the default of
        // `allow_unpress = false`. Membership still works either way, because
        // that is keyed on the reference text rather than on the body.
        for tmpl in &mut templates {
            let Some(group_id) = tmpl.properties.get("button_group").and_then(|v| match v {
                Variant::String(s) => s.strip_prefix("SubResource:").map(String::from),
                _ => None,
            }) else {
                continue;
            };
            if let Some(group_props) = sub_resource_props.get(&group_id) {
                for (key, value) in group_props {
                    tmpl.properties
                        .insert(format!("__button_group__{key}"), value.clone());
                }
            }
        }

        // The same flatten for a `FoldableContainer`'s `FoldableGroup`
        // (pat-s3rrv.165). `control.rs` reads `allow_folding_all` from a
        // `__foldable_group__*` companion and nothing ever wrote one, so the flag
        // was unreachable at runtime in exactly the way `allow_unpress` was above,
        // with the same silent symptom: a group authored to let every member close
        // still refused to fold its last open one, because the reader saw the
        // default `false`.
        //
        // Membership itself never depended on this — `foldable_group_of` keys on the
        // reference text — so only the body-borne flag was lost, which is why
        // exclusivity looked correct while `allow_folding_all` did nothing.
        //
        // ⚠️ INLINE groups only, as above: a `.tres` FoldableGroup shared between
        // scenes arrives as `ExtResource`, whose body lives in a file this pass never
        // parses, and keeps the default.
        for tmpl in &mut templates {
            let Some(group_id) = tmpl.properties.get("foldable_group").and_then(|v| match v {
                Variant::String(s) => s.strip_prefix("SubResource:").map(String::from),
                _ => None,
            }) else {
                continue;
            };
            if let Some(group_props) = sub_resource_props.get(&group_id) {
                for (key, value) in group_props {
                    tmpl.properties
                        .insert(format!("__foldable_group__{key}"), value.clone());
                }
            }
        }

        // Resolve a mesh's material albedo to an `albedo_color` property so the
        // 3D viewport can shade the mesh in its real colour (like Godot), from a
        // node-level `surface_material_override/0` or the mesh's own `material`.
        // Run before mesh resolution, while `mesh = SubResource:<id>` is intact.
        let sub_ref_id = |v: Option<&Variant>| -> Option<String> {
            match v {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").map(String::from),
                _ => None,
            }
        };
        // Flatten a Line2D's `gradient` and `width_curve` sub-resources onto the
        // node (pat-s3rrv.85). By render time the `PackedScene` — and with it
        // `sub_resource_props` — is gone; only the `SceneTree` remains, so the
        // per-vertex colour and width the rasterizer needs have to travel on the
        // node, exactly as `__material__*` and `curve_points` do.
        for tmpl in &mut templates {
            if let Some(grad_id) = sub_ref_id(tmpl.properties.get("gradient")) {
                if let Some(props) = sub_resource_props.get(&grad_id) {
                    // Godot's Gradient stores parallel stop arrays; both are
                    // required to sample, so a bag missing either is left alone
                    // rather than half-copied.
                    if let (Some(offsets), Some(colors)) =
                        (props.get("offsets"), props.get("colors"))
                    {
                        tmpl.properties
                            .insert("__gradient_offsets".into(), offsets.clone());
                        tmpl.properties
                            .insert("__gradient_colors".into(), colors.clone());
                    }
                }
            }
            if let Some(curve_id) = sub_ref_id(tmpl.properties.get("width_curve")) {
                if let Some(Variant::Array(data)) = sub_resource_props
                    .get(&curve_id)
                    .and_then(|p| p.get("_data"))
                {
                    if let Some(samples) = bake_curve_samples(&data.to_vec()) {
                        tmpl.properties.insert(
                            "__width_curve_samples".into(),
                            Variant::PackedFloat32Array(samples),
                        );
                    }
                }
            }
        }

        for tmpl in &mut templates {
            // Godot's resolve order, from the renderer itself:
            // `render_forward_clustered.cpp:4206` picks
            // `material_override.is_valid() ? material_override : p_material`,
            // and doc/classes/GeometryInstance3D.xml:67 says the override "will
            // be used instead of any material set in any material slot of the
            // mesh". So `material_override` wins outright, the per-surface slot
            // is next, and the mesh's own material is the fallback.
            //
            // `material_override` used to be missing from this chain entirely —
            // the comment here claimed the ShaderMaterial pass below handled it,
            // but that pass only accepts sub-resources whose class is literally
            // `ShaderMaterial`. A node overriding with a plain
            // StandardMaterial3D therefore matched NEITHER pass and rendered
            // unmaterialed: no `__material__*` companions, so
            // `material3d_for_node` answered `None` and the mesh fell back to
            // flat shading with the authored colour, metallic, roughness and
            // render modes all silently dropped (pat-s3rrv.30).
            //
            // ⚠️ ShaderMaterial is excluded here deliberately. The pass below
            // owns that case and flattens it to `shader_code` /
            // `shader_parameter/*`; letting it through would additionally give
            // the node a `__material__*` bag holding none of the
            // StandardMaterial3D keys, so `material3d_for_node` would hand the
            // renderer a DEFAULT material where it correctly returns `None`
            // today — turning every shader-material mesh flat.
            let override_id = sub_ref_id(tmpl.properties.get("material_override"))
                .filter(|id| sub_resources.get(id).map(String::as_str) != Some("ShaderMaterial"));
            let mat_id = override_id.or_else(|| {
                sub_ref_id(tmpl.properties.get("surface_material_override/0")).or_else(|| {
                    sub_ref_id(tmpl.properties.get("mesh"))
                        .and_then(|mesh_id| sub_resource_props.get(&mesh_id))
                        .and_then(|p| sub_ref_id(p.get("material")))
                })
            });
            // pat-mgixk.53: an `ExtResource` material, flattened the same way.
            //
            // Tried only when no SubResource matched, so an inline material
            // keeps precedence exactly as before — this fills a gap rather than
            // changing any case that already worked.
            //
            // ⚠️ Scoped to the two properties that flatten onto `__material__*`,
            // which is the only material bag the renderer reads. `material_overlay`
            // and `surface_material_override/N` for N > 0 are deliberately NOT
            // flattened here: there is no `__overlay__`/`__surfmat` ExtResource
            // reader on the render side, so inventing a prefix nothing consumes
            // would be wired-but-dead code that looks like support. Worth its own
            // bead once those readers exist.
            if mat_id.is_none() {
                if let Some((class, props)) =
                    ext_material_props(tmpl.properties.get("material_override"), &ext_resources)
                        .or_else(|| {
                            ext_material_props(
                                tmpl.properties.get("surface_material_override/0"),
                                &ext_resources,
                            )
                        })
                {
                    if let Some(Variant::Color(c)) = props
                        .iter()
                        .find(|(k, _)| k == "albedo_color")
                        .map(|(_, v)| v)
                    {
                        tmpl.properties
                            .insert("albedo_color".into(), Variant::Color(*c));
                    }
                    tmpl.properties
                        .insert("__material__class".into(), Variant::String(class));
                    for (key, value) in props {
                        tmpl.properties.insert(format!("__material__{key}"), value);
                    }
                }
            }
            if let Some(mat_id) = mat_id {
                if let Some(Variant::Color(c)) = sub_resource_props
                    .get(&mat_id)
                    .and_then(|p| p.get("albedo_color"))
                {
                    tmpl.properties
                        .insert("albedo_color".into(), Variant::Color(*c));
                }

                // Copy the material sub-resource's FULL property bag onto the
                // node under `__material__*`, mirroring the `__shape__*` pattern
                // above. By render time the `PackedScene` — and with it
                // `sub_resource_props` — is gone; only the `SceneTree` remains,
                // so anything the renderer needs has to travel on the node.
                // Without this only `albedo_color` survived and every authored
                // material rendered as a flat colour with metallic, roughness,
                // emission and its textures silently dropped (pat-0q2kx.42).
                //
                // Copying verbatim keeps this free of per-material knowledge, the
                // same way the shape pass avoids knowing about `radius` vs `size`.
                if let Some(mat_props) = sub_resource_props.get(&mat_id) {
                    let class = sub_resources.get(&mat_id).cloned().unwrap_or_default();
                    tmpl.properties
                        .insert("__material__class".into(), Variant::String(class));
                    for (key, value) in mat_props {
                        tmpl.properties
                            .insert(format!("__material__{key}"), value.clone());
                    }
                }
            }
        }

        // Flatten a node-level `CanvasItemMaterial` onto the node under
        // `__canvas_material__*` (pat-0q2kx.131).
        //
        // The ShaderMaterial pass below/above deliberately filters `material` to
        // `ShaderMaterial`, and the `__material__*` pass only ever looks at a 3D
        // mesh's material — so a 2D node carrying `material =
        // SubResource("CanvasItemMaterial_x")` matched neither and its blend
        // mode was dropped at load. Same companion-property mechanism as
        // `__shape__*`: by render time the sub-resource is gone and only the
        // `SceneTree` remains.
        for tmpl in &mut templates {
            let Some(mat_id) = sub_ref_id(tmpl.properties.get("material")) else {
                continue;
            };
            if sub_resources.get(&mat_id).map(String::as_str) != Some("CanvasItemMaterial") {
                continue;
            }
            let Some(mat_props) = sub_resource_props.get(&mat_id) else {
                continue;
            };
            // Verbatim, so this stays free of per-property knowledge — the same
            // reasoning as the shape and 3D-material passes.
            for (key, value) in mat_props {
                tmpl.properties
                    .insert(format!("__canvas_material__{key}"), value.clone());
            }
            // A CanvasItemMaterial with every property left at its default has
            // an empty property bag, so record its presence explicitly rather
            // than inferring it from the companions.
            tmpl.properties
                .insert("__canvas_material__present".into(), Variant::Bool(true));
        }

        // Flatten a `PhysicsMaterial` override onto the body node
        // (pat-0q2kx.115).
        //
        // Godot bodies have no flat `bounce`/`friction`: both live on a
        // `PhysicsMaterial` resource assigned to `physics_material_override`
        // (upstream/godot/scene/resources/physics_material.cpp). The physics
        // bridge cannot reach `sub_resource_props` — by the time it runs, the
        // `PackedScene` is gone — so the values travel on the node under
        // `__physics_material__*`, mirroring the `__material__*` / `__shape__*`
        // passes.
        //
        // Only keys the resource actually carries are written, so an override
        // that sets `bounce` alone does not silently reset `friction`.
        for tmpl in &mut templates {
            let Some(mat_id) = sub_ref_id(tmpl.properties.get("physics_material_override")) else {
                continue;
            };
            if sub_resources.get(&mat_id).map(String::as_str) != Some("PhysicsMaterial") {
                continue;
            }
            let Some(props) = sub_resource_props.get(&mat_id) else {
                continue;
            };
            for key in ["friction", "bounce", "rough", "absorbent"] {
                if let Some(value) = props.get(key) {
                    tmpl.properties
                        .insert(format!("__physics_material__{key}"), value.clone());
                }
            }
        }

        // Flatten a `ParticleProcessMaterial` onto the emitter node
        // (pat-s3rrv.39).
        //
        // Godot puts a particle system's motion parameters on a
        // `ParticleProcessMaterial` sub-resource assigned to `process_material`,
        // not on the node: `gravity`, `direction`, `spread`,
        // `initial_velocity_min/max` and `color` all live there
        // (upstream/godot/scene/resources/particle_process_material.cpp).
        //
        // `MainLoop`'s emitter setup already reads every one of those as a PLAIN
        // node property — the comment at main_loop.rs:677 even says the flattener
        // "lands [them] on the node ... so they are read the same way as every
        // other material parameter". Nothing ever produced them: there was a
        // flattener for `material_override`, `surface_material_override/0`,
        // `physics_material_override`, SpriteFrames and TileSet, but none for
        // `process_material`. So a Godot-authored particle scene parsed fine and
        // then emitted with default motion — no gravity, no aimed direction.
        //
        // Unprefixed keys (not `__particle_material__*`) because that is exactly
        // what the emitter reads. Only keys the resource carries are written, and
        // never over a value already on the node: a scene or a hand-built test
        // that sets `gravity` directly keeps winning, matching how the other
        // flatteners avoid silently resetting unspecified fields.
        for tmpl in &mut templates {
            let Some(mat_id) = sub_ref_id(tmpl.properties.get("process_material")) else {
                continue;
            };
            if sub_resources.get(&mat_id).map(String::as_str) != Some("ParticleProcessMaterial") {
                continue;
            }
            let Some(props) = sub_resource_props.get(&mat_id) else {
                continue;
            };
            // ⚠️ EVERY key the resource carries, not a hard-coded list.
            //
            // The list here was exactly eight keys (direction, gravity, spread,
            // initial_velocity_min/max, color, scale_min/max) while `MainLoop`
            // reads a dozen more straight off the node — `sub_emitter_mode`,
            // `sub_emitter_frequency`, `sub_emitter_amount_at_end`,
            // `sub_emitter_keep_velocity`, `collision_mode`,
            // `collision_friction`, `collision_bounce`,
            // `attractor_interaction_enabled` — with comments asserting that
            // this flattener lands them there. It did not. Godot saves all of
            // them on the `[sub_resource type="ParticleProcessMaterial"]`, so an
            // editor-authored scene's sub-emitters, collision response and
            // attractor opt-out never engaged. The only things that ever set
            // them were tests authoring them directly on the NODE, which is why
            // the gap read as done (pat-mgixk.68).
            //
            // Copying whatever the resource declares fixes the whole class at
            // once and cannot drift: `ParticleProcessMaterial` has ~113
            // properties, and an allowlist goes stale every time a reader starts
            // consulting one more.
            //
            // ⚠️ Resource-typed values are SKIPPED. A `SubResource:<id>` string
            // is a reference into `sub_resource_props`, which is gone by render
            // time — landing it on the node would hand a reader an id it cannot
            // resolve. Those need the two-hop sampling that `color_ramp` and the
            // `*_curve` params get below.
            //
            // `resource_*` is skipped too: those are `Resource` bookkeeping
            // (`resource_name`, `resource_local_to_scene`) that a `.tscn`
            // sub_resource may carry, and they describe the material rather than
            // the particles.
            for (key, value) in props {
                if tmpl.properties.contains_key(key) {
                    continue;
                }
                if key.starts_with("resource_") {
                    continue;
                }
                if sub_ref_id(Some(value)).is_some() {
                    continue;
                }
                tmpl.properties.insert(key.clone(), value.clone());
            }

            // `color_ramp` is what makes a particle's colour change over its
            // lifetime, and it was never parsed — the emitter fell back to the
            // flat `color` (pat-s3rrv.116).
            //
            // 🔑 The stops are TWO hops away. Godot types `color_ramp` as
            // **Texture2D**, not Gradient (`ParticleProcessMaterial.xml`): the
            // editor assigns a GradientTexture1D whose own `gradient` property
            // holds the Gradient with the `offsets`/`colors` arrays
            // (`GradientTexture1D.xml`). Reading `offsets` off the ramp itself
            // finds nothing.
            //
            // ⚠️ A direct Gradient is accepted too, since a hand-written scene
            // may skip the texture wrapper. The two are told apart by shape
            // rather than by class name: the bag carrying `offsets` is the
            // Gradient, wherever it sits.
            if let Some(ramp_id) = sub_ref_id(props.get("color_ramp")) {
                let stops = sub_resource_props.get(&ramp_id).and_then(|ramp| {
                    if ramp.contains_key("offsets") {
                        Some(ramp)
                    } else {
                        sub_ref_id(ramp.get("gradient"))
                            .and_then(|gid| sub_resource_props.get(&gid))
                    }
                });
                // Both arrays are required to sample, so a bag missing either is
                // left alone rather than half-copied.
                if let Some((offsets, colors)) =
                    stops.and_then(|st| Some((st.get("offsets")?, st.get("colors")?)))
                {
                    tmpl.properties
                        .insert("__color_ramp_offsets".into(), offsets.clone());
                    tmpl.properties
                        .insert("__color_ramp_colors".into(), colors.clone());
                }
            }

            // `scale_curve` scales a particle over its lifetime. Same two-hop
            // shape as `color_ramp` above: Godot types it **Texture2D**
            // (`set_param_texture`), so the editor assigns a CurveTexture whose
            // own `curve` holds the Curve carrying `_data`.
            //
            // ⚠️ A direct Curve is accepted as well, told apart by shape — the
            // bag carrying `_data` is the Curve wherever it sits — matching how
            // `color_ramp` handles its wrapper.
            //
            // The samples are baked here with the same helper Line2D's
            // `width_curve` uses, so both spellings of "a Curve in a .tscn"
            // agree on what the curve means.
            if let Some(curve_ref) = sub_ref_id(props.get("scale_curve")) {
                let data = sub_resource_props.get(&curve_ref).and_then(|bag| {
                    if bag.contains_key("_data") {
                        Some(bag)
                    } else {
                        sub_ref_id(bag.get("curve")).and_then(|cid| sub_resource_props.get(&cid))
                    }
                });
                if let Some(Variant::Array(data)) = data.and_then(|b| b.get("_data")) {
                    if let Some(samples) = bake_curve_samples(&data.to_vec()) {
                        tmpl.properties.insert(
                            "__scale_curve_samples".into(),
                            Variant::PackedFloat32Array(samples),
                        );
                    }
                }
            }
        }

        // Bake a `Curve2D`/`Curve3D` sub-resource into the polyline the path
        // driver samples (pat-0q2kx.104).
        //
        // Godot serializes the curve as `_data = { "points": PackedVector2Array }`
        // where every three entries are one control point — `in` handle, `out`
        // handle, then position, in that order (upstream/godot/scene/resources/
        // curve.cpp `Curve2D::_set_data`). Handles are relative to the position.
        //
        // `process_path_follows` has always read a `curve_points` polyline off
        // the Path2D node, but nothing produced it from a real scene: a
        // Godot-authored `curve = SubResource("Curve2D_…")` was parsed and
        // dropped, so PathFollow2D was inert outside hand-built tests. Baking
        // here — rather than at sample time — matches Godot, which bakes on load
        // and samples the cache, and it keeps `curve_points` as the storage key
        // so the existing driver needs no change.
        for tmpl in &mut templates {
            let Some(curve_id) = sub_ref_id(tmpl.properties.get("curve")) else {
                continue;
            };
            let class = sub_resources.get(&curve_id).map(String::as_str);
            if class == Some("Curve3D") {
                // The 3D half (pat-s3rrv.83). `Curve3D::_set_data` takes the
                // SAME in/out/position triple as the 2D form, but over a
                // `PackedVector3Array` (`curve.cpp:2278-2299`), so only the
                // element type differs — the ordering does not.
                //
                // ⚠️ Godot's `_set_data` also requires a `tilts` array and
                // ERR_FAILs without it. Tilt rotates PathFollow3D about the
                // curve; it does not move the polyline, so it is deliberately
                // not read here — baking the path and rotating along it are
                // separate concerns, and inventing a tilt would be worse than
                // leaving rotation at its default.
                let Some(Variant::Dictionary(data)) = sub_resource_props
                    .get(&curve_id)
                    .and_then(|p| p.get("_data"))
                else {
                    continue;
                };
                let Some(Variant::PackedVector3Array(raw)) = data.get("points") else {
                    continue;
                };
                if raw.is_empty() || raw.len() % 3 != 0 {
                    continue;
                }
                let mut control: Vec<crate::bezier_bake::CurvePoint3> = raw
                    .chunks_exact(3)
                    .map(|t| crate::bezier_bake::CurvePoint3 {
                        in_handle: t[0],
                        out_handle: t[1],
                        position: t[2],
                    })
                    .collect();
                // `closed` loops the last point back to the first — but ONLY
                // "if the curve has more than 2 control points"
                // (doc/classes/Curve3D.xml:207). A 2-point curve marked closed
                // stays open in Godot, so the count guard is part of the rule
                // rather than a defensive extra.
                //
                // Appending a copy of the first point is the whole
                // implementation: `bake_curve_3d` walks `windows(2)`, so the new
                // final pair is (last, first) and picks up `last.out_handle`
                // with `first.in_handle` exactly as any other segment would.
                let closed = matches!(
                    sub_resource_props
                        .get(&curve_id)
                        .and_then(|p| p.get("closed")),
                    Some(Variant::Bool(true))
                );
                // Carry the AUTHORED control points onto the node, so the
                // runtime `Curve3D` can rebuild the bezier the scene described
                // rather than only the polyline baked from it. `sample`,
                // `samplef` and `tessellate` all evaluate handles, which the
                // baked polyline has already thrown away (pat-s3rrv.91 item 5).
                //
                // ⚠️ Written from `raw`, BEFORE the `closed` push below. Godot's
                // `_data["points"]` does NOT contain the duplicated first point
                // — `closed` is a separate flag and `sample`/`tessellate` add
                // the wrap segment themselves. Writing the post-push array
                // alongside `curve_closed = true` would make them wrap a second
                // time, appending a closing segment the author never drew.
                tmpl.properties.insert(
                    "curve_control_points".into(),
                    Variant::PackedVector3Array(raw.to_vec()),
                );
                tmpl.properties
                    .insert("curve_closed".into(), Variant::Bool(closed));

                if closed && control.len() > 2 {
                    control.push(control[0]);
                }

                // `bake_interval` is "the distance in meters between two
                // adjacent cached points" (`doc/classes/Curve3D.xml:204`), and
                // every baker below has to agree on it: the polyline, the tilts
                // and the up vectors are read INDEX-FOR-INDEX by the runtime, so
                // the subdivision is computed ONCE here and handed to all three.
                // `bake_curve_3d_tilts` takes no geometry and could not derive
                // it even if it wanted to.
                //
                // ⚠️ Resolved AFTER the `closed` push so the wrap segment is
                // subdivided too — a curve that loops back across the whole path
                // has its longest span in exactly that pair.
                //
                // ⚠️ An `Int` arm as well as a `Float` one: Godot writes whole
                // numbers unquoted, so a hand-edited `bake_interval = 1` arrives
                // as `Variant::Int`.
                //
                // ⚠️ A non-positive interval falls back to the class default
                // rather than through to the baker. `even_length_schedule_3d`
                // reads `<= 0.0` as "no subdivision" — correct for a caller that
                // asked for it directly, but a scene file carrying `0.0` has an
                // out-of-range value, not a request for a 2-point path, and
                // silently baking one would drop the curve to its chords.
                let bake_interval = match sub_resource_props
                    .get(&curve_id)
                    .and_then(|p| p.get("bake_interval"))
                {
                    Some(Variant::Float(f)) if *f > 0.0 => *f as f32,
                    Some(Variant::Int(i)) if *i > 0 => *i as f32,
                    _ => crate::bezier_bake::CURVE3D_BAKE_INTERVAL,
                };
                let schedule = crate::bezier_bake::even_length_schedule_3d(
                    &control,
                    crate::bezier_bake::MAX_BAKE_STAGES,
                    bake_interval,
                );

                let baked = crate::bezier_bake::bake_curve_3d(&control, &schedule);
                if baked.is_empty() {
                    continue;
                }
                tmpl.properties
                    .insert("curve_points".into(), Variant::PackedVector3Array(baked));

                // Per-point tilt, which `_set_data` requires and Godot always
                // writes (`curve.cpp:2278-2299`). An earlier pass deliberately
                // skipped it — "inventing a tilt would be worse than [none]" —
                // which was right while nothing consumed a tilt; PathFollow3D
                // now twists about the tangent by it, so the value has to
                // travel with the polyline.
                //
                // ⚠️ Baked to the SAME cadence as the points and carried as a
                // parallel array, so `curve_points[i]` and `curve_tilts[i]`
                // describe one place on the curve. A tilt array that disagreed
                // in length with the polyline would rotate the follower by an
                // angle belonging to somewhere else on the path — plausible
                // motion, wrong everywhere.
                if let Some(Variant::PackedFloat32Array(raw_tilts)) = data.get("tilts") {
                    let mut tilts = raw_tilts.clone();
                    // Mirrors the `closed` handling above: the appended copy of
                    // the first control point needs its tilt appended too, or
                    // the two arrays end at different lengths.
                    if closed && tilts.len() > 2 && control.len() == tilts.len() + 1 {
                        tilts.push(tilts[0]);
                    }
                    if tilts.len() == control.len() {
                        let baked_tilts =
                            crate::bezier_bake::bake_curve_3d_tilts(&tilts, &schedule);
                        tmpl.properties.insert(
                            "curve_tilts".into(),
                            Variant::PackedFloat32Array(baked_tilts),
                        );
                    }
                }

                // Per-sample UP VECTORS by parallel transport, which is what
                // gives a PathFollow3D its roll on a curve that climbs or banks
                // (`curve.cpp:1785-1822`).
                //
                // ⚠️ `up_vector_enabled` DEFAULTS TO TRUE
                // (`doc/classes/Curve3D.xml`), so an absent property must bake
                // them. Godot represents the disabled case by clearing the
                // cache, so the property is simply absent here too and every
                // consumer falls back to world up.
                let up_vectors_enabled = !matches!(
                    sub_resource_props
                        .get(&curve_id)
                        .and_then(|p| p.get("up_vector_enabled")),
                    Some(Variant::Bool(false))
                );
                if up_vectors_enabled {
                    let ups = crate::bezier_bake::bake_curve_3d_up_vectors(&control, &schedule);
                    if !ups.is_empty() {
                        tmpl.properties
                            .insert("curve_up_vectors".into(), Variant::PackedVector3Array(ups));
                    }
                }
                continue;
            }
            if class != Some("Curve2D") {
                continue;
            }
            let Some(Variant::Dictionary(data)) = sub_resource_props
                .get(&curve_id)
                .and_then(|p| p.get("_data"))
            else {
                continue;
            };
            let Some(Variant::PackedVector2Array(raw)) = data.get("points") else {
                continue;
            };
            // A malformed array is skipped rather than truncated: a partial
            // triple would silently bake a curve with a wrong last segment.
            if raw.is_empty() || raw.len() % 3 != 0 {
                continue;
            }
            let control: Vec<crate::bezier_bake::CurvePoint> = raw
                .chunks_exact(3)
                .map(|t| crate::bezier_bake::CurvePoint {
                    in_handle: t[0],
                    out_handle: t[1],
                    position: t[2],
                })
                .collect();
            // `bake_interval` is "the distance in pixels between two adjacent
            // cached points" (`doc/classes/Curve2D.xml:174`), the 2D twin of the
            // Curve3D handling above.
            //
            // ⚠️ The default is 5.0 PIXELS, twenty-five times the 3D default of
            // 0.2 metres. The two constants are separate for that reason —
            // sharing one would resample every 2D path at a metre-scale density.
            //
            // ⚠️ A uniform step count CANNOT honour this, which is what the old
            // literal 16 here was: a zero-handle segment is a smoothstep that
            // crosses its own chord at 1.5x speed, so the midpoint gap
            // overshoots whatever count is chosen (pat-s3rrv.83's measurement:
            // 0.05859232 = 15/256 for a requested 0.05).
            let bake_interval = match sub_resource_props
                .get(&curve_id)
                .and_then(|p| p.get("bake_interval"))
            {
                Some(Variant::Float(f)) if *f > 0.0 => *f as f32,
                Some(Variant::Int(i)) if *i > 0 => *i as f32,
                _ => crate::bezier_bake::CURVE2D_BAKE_INTERVAL,
            };
            let schedule = crate::bezier_bake::even_length_schedule_2d(
                &control,
                crate::bezier_bake::MAX_BAKE_STAGES,
                bake_interval,
            );
            let baked = crate::bezier_bake::bake_curve(&control, &schedule);
            if baked.is_empty() {
                continue;
            }
            tmpl.properties.insert(
                "curve_points".into(),
                Variant::PackedVector2Array(baked.iter().map(|b| b.position).collect()),
            );
        }

        // Unflatten a `SpriteFrames` sub-resource back into the `__sf_*`
        // companion keys the runtime and the editor panel read (pat-0q2kx.74).
        //
        // Godot stores the resource as `animations = [{frames, loop, name,
        // speed}]` with each frame an `{duration, texture}` dictionary. By the
        // time anything animates, the `PackedScene` — and with it
        // `sub_resource_props` — is gone, so the frame list has to travel on the
        // node, mirroring the `__material__*` and `__shape__*` passes above.
        //
        // Anything that does not match the expected shape is skipped rather than
        // guessed at, so a hand-written or future-Godot variation degrades to
        // "no frames" instead of a wrong frame count.
        for tmpl in &mut templates {
            let Some(sf_id) = sub_ref_id(tmpl.properties.get("sprite_frames")) else {
                continue;
            };
            if sub_resources.get(&sf_id).map(String::as_str) != Some("SpriteFrames") {
                continue;
            }
            let Some(Variant::Array(anims)) = sub_resource_props
                .get(&sf_id)
                .and_then(|p| p.get("animations"))
            else {
                continue;
            };

            let mut names: Vec<String> = Vec::new();
            for entry in anims.to_vec().iter() {
                let Variant::Dictionary(dict) = entry else {
                    continue;
                };
                // The name is a StringName (`&"run"`) in Godot's output, but a
                // plain String is accepted too — both spell the same thing.
                let name = match dict.get("name") {
                    Some(Variant::StringName(sn)) => sn.as_str().to_string(),
                    Some(Variant::String(s)) => s.clone(),
                    _ => continue,
                };
                let speed = match dict.get("speed") {
                    Some(Variant::Float(f)) => f,
                    Some(Variant::Int(i)) => i as f64,
                    _ => 5.0,
                };
                let loops = !matches!(dict.get("loop"), Some(Variant::Bool(false)));
                let mut frames: Vec<String> = Vec::new();
                if let Some(Variant::Array(list)) = dict.get("frames") {
                    for f in list.to_vec() {
                        let Variant::Dictionary(fd) = f else { continue };
                        // The texture is an `ExtResource:<id>` reference; resolve
                        // it to the path the renderer can actually load.
                        let Some(Variant::String(reference)) = fd.get("texture") else {
                            continue;
                        };
                        let Some(ext_id) = reference.strip_prefix("ExtResource:") else {
                            continue;
                        };
                        if let Some(entry) = ext_resources.get(ext_id) {
                            frames.push(entry.path.clone());
                        }
                    }
                }
                tmpl.properties
                    .insert(format!("__sf_{name}_speed"), Variant::Float(speed));
                tmpl.properties
                    .insert(format!("__sf_{name}_loop"), Variant::Bool(loops));
                tmpl.properties.insert(
                    format!("__sf_{name}_frames"),
                    Variant::PackedStringArray(frames),
                );
                names.push(name);
            }
            if !names.is_empty() {
                tmpl.properties
                    .insert("__sf_anims__".into(), Variant::PackedStringArray(names));
            }
        }

        // Resolve a ShaderMaterial referenced by `material_override` or
        // `surface_material_override/0` into flat node properties the 3D render
        // server understands (`shader_code` / `shader_path`, `shader_type`, and
        // `shader_parameter/*`). The ShaderMaterial's `shader` field points to
        // either an inline `[sub_resource type="Shader"]` (with a `code`
        // property — fully resolved here) or an external `.gdshader`
        // `[ext_resource]` (path recorded under `shader_path` for the render
        // server to load). Mirrors Godot's ShaderMaterial → Shader chain.
        for tmpl in &mut templates {
            let ext_ref_id = |v: Option<&Variant>| -> Option<String> {
                match v {
                    Some(Variant::String(s)) => s.strip_prefix("ExtResource:").map(String::from),
                    _ => None,
                }
            };
            // Find the ShaderMaterial sub-resource id (override or per-surface).
            let smat_id = sub_ref_id(tmpl.properties.get("material_override"))
                .or_else(|| sub_ref_id(tmpl.properties.get("surface_material_override/0")))
                .filter(|id| sub_resources.get(id).map(String::as_str) == Some("ShaderMaterial"))
                // A CanvasItem carries its ShaderMaterial on the plain `material`
                // property — Sprite2D, ColorRect, TextureRect and friends have no
                // 3D override slot, so the two lookups above never found it and
                // every authored 2D shader was dropped at load (pat-0q2kx.48).
                // Filtered separately, and last, so 3D resolution is untouched.
                .or_else(|| {
                    sub_ref_id(tmpl.properties.get("material")).filter(|id| {
                        sub_resources.get(id).map(String::as_str) == Some("ShaderMaterial")
                    })
                });
            let Some(smat_id) = smat_id else { continue };
            let Some(smat_props) = sub_resource_props.get(&smat_id) else {
                continue;
            };

            // Resolve the shader source: inline Shader sub-resource `code`, or an
            // external `.gdshader` path via ext_resource.
            if let Some(shader_id) = sub_ref_id(smat_props.get("shader")) {
                if let Some(Variant::String(code)) = sub_resource_props
                    .get(&shader_id)
                    .and_then(|p| p.get("code"))
                {
                    let shader = gdresource::importers::shader_from_source(code, "");
                    tmpl.properties
                        .insert("shader_code".into(), Variant::String(code.clone()));
                    if let Some(gdvariant::Variant::String(ty)) = shader.get_property("shader_type")
                    {
                        tmpl.properties
                            .insert("shader_type".into(), Variant::String(ty.clone()));
                    }
                }
            } else if let Some(ext_id) = ext_ref_id(smat_props.get("shader")) {
                if let Some(entry) = ext_resources.get(&ext_id) {
                    tmpl.properties
                        .insert("shader_path".into(), Variant::String(entry.path.clone()));
                }
            }

            // Copy every `shader_parameter/*` entry from the ShaderMaterial onto
            // the node so the render server can bind uniform values.
            let params: Vec<(String, Variant)> = smat_props
                .iter()
                .filter(|(k, _)| k.starts_with("shader_parameter/"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (k, v) in params {
                tmpl.properties.insert(k, v);
            }
        }

        // Resolve every `prop = ExtResource("id")` reference to the referenced
        // resource's path, stored under an `__extres__<prop>` companion
        // (pat-0q2kx.6).
        //
        // Godot's `[ext_resource]` header carries the `path` for each id, and a
        // node property that names one (`stream`, `mesh`, `texture`, …) is a
        // `Ref<Resource>` to that file. Until now only `script` was resolved, so
        // every other consumer saw the literal string `ExtResource("id")` and
        // silently failed to load anything — an `AudioStreamPlayer` in a genuine
        // Godot scene was mute for exactly this reason.
        //
        // The companion key (rather than overwriting `prop`) keeps the raw ref
        // intact for scene round-trip and the inspector, matching how
        // `__viewport_mesh__` / `__resref__<prop>` already work.
        for tmpl in &mut templates {
            let resolved: Vec<(String, String, Option<String>)> = tmpl
                .properties
                .iter()
                .filter_map(|(k, v)| {
                    let s = match v {
                        Variant::String(s) => s,
                        _ => return None,
                    };
                    let id = parse_ext_resource_ref(s)?;
                    let entry = ext_resources.get(&id)?;
                    // The uid rides along as its own companion so the saver can
                    // re-emit it; `__extres__` alone only carries the path
                    // (pat-0q2kx.137).
                    Some((k.clone(), entry.path.clone(), entry.uid.clone()))
                })
                .collect();
            for (key, path, uid) in resolved {
                tmpl.properties
                    .insert(format!("__extres__{key}"), Variant::String(path));
                if let Some(uid) = uid {
                    tmpl.properties
                        .insert(format!("__extuid__{key}"), Variant::String(uid));
                }
            }
        }

        // Resolve an `AtlasTexture` assigned to a node's `texture`
        // (pat-s3rrv.117).
        //
        // Godot's AtlasTexture presents a sub-rect of a sheet as a standalone
        // texture — the backbone of sprite-sheet workflows. A Sprite2D given one
        // drew the WHOLE sheet, because `node2d::get_texture_path` found a
        // `SubResource(...)` where it wanted a path and gave up.
        //
        // 🔑 An AtlasTexture is a REDIRECT plus a rect, so it needs both halves:
        // the drawn image is the `atlas`, which is an ExtResource pointing at
        // the sheet file, and the sub-rect is the AtlasTexture's own `region`.
        // Flattening only the region would leave the texture unresolved; only
        // the path would draw the entire sheet.
        //
        // ⚠️ `__extres__texture` is written deliberately, even though `texture`
        // holds a SubResource rather than an ExtResource. That companion is
        // defined as "the resolved path for this property", which is exactly
        // what the sheet's path is, and `get_texture_path` already prefers it.
        // The `texture` property itself keeps its `SubResource:<id>` value, so
        // the scene still round-trips and the sub-resource body is still
        // emitted from `__resref__`.
        for tmpl in &mut templates {
            let Some(atlas_id) = sub_ref_id(tmpl.properties.get("texture")) else {
                continue;
            };
            if sub_resources.get(&atlas_id).map(String::as_str) != Some("AtlasTexture") {
                continue;
            }
            let Some(props) = sub_resource_props.get(&atlas_id) else {
                continue;
            };

            // The sheet the region is cut from.
            if let Some(path) = props.get("atlas").and_then(|v| match v {
                Variant::String(s) => {
                    let id = parse_ext_resource_ref(s)?;
                    Some(ext_resources.get(&id)?.path.clone())
                }
                _ => None,
            }) {
                tmpl.properties
                    .insert("__extres__texture".into(), Variant::String(path));
            }

            // `region` is what the sprite must sample; `margin` and
            // `filter_clip` ride along so the draw path can honour them without
            // a second lookup.
            for (key, companion) in [
                ("region", "__atlas_region"),
                ("margin", "__atlas_margin"),
                ("filter_clip", "__atlas_filter_clip"),
            ] {
                if let Some(value) = props.get(key) {
                    tmpl.properties.insert(companion.to_string(), value.clone());
                }
            }
        }

        // Resolve `mesh = SubResource("id")` references to a concrete primitive
        // marker (`primitive:<Type>`) the 3D viewport renderer understands, stored
        // under a `__viewport_mesh__` companion so the real `mesh` property keeps
        // its `SubResource:<id>` value (correct scene round-trip + inspector).
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("mesh") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").and_then(|id| {
                    sub_resources.get(id).map(|ty| {
                        let query = primitive_marker_query(sub_resource_props.get(id));
                        (id.to_string(), ty.clone(), format!("primitive:{ty}{query}"))
                    })
                }),
                _ => None,
            };
            if let Some((mesh_id, mesh_class, p)) = resolved {
                tmpl.properties
                    .insert("__viewport_mesh__".into(), Variant::String(p));
                // The marker carries only the TYPE, so an authored
                // `inner_radius`/`size`/`rings` on the sub-resource was dropped
                // and every primitive rendered at its default size
                // (pat-s3rrv.199). Copy the sub-resource's class and typed
                // properties onto the node under `__mesh__*`, exactly as the
                // `__shape__*` pass below does for collision geometry — by
                // render time the sub-resource itself is gone.
                //
                // Verbatim, with no per-mesh knowledge here: `radius`, `size`,
                // `rings`, `ring_segments`, `left_to_right` all come along, and
                // a resolver that wants `size.x` splits the stored vector on its
                // own side.
                tmpl.properties
                    .insert("__mesh__class".into(), Variant::String(mesh_class));
                if let Some(mesh_props) = sub_resource_props.get(&mesh_id) {
                    for (key, value) in mesh_props {
                        tmpl.properties
                            .insert(format!("__mesh__{key}"), value.clone());
                    }
                }
            }
        }

        // Resolve `shape = SubResource("id")` on CollisionShape nodes to a
        // `shape:<Type>` marker so the 3D viewport can draw the matching
        // collision-shape wireframe gizmo (BoxShape3D, SphereShape3D, etc.),
        // stored under a `__viewport_shape__` companion so the real `shape`
        // property keeps its `SubResource:<id>` value (round-trip + inspector).
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("shape") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").and_then(|id| {
                    let ty = sub_resources.get(id)?;
                    Some((
                        id.to_string(),
                        ty.clone(),
                        encode_shape_marker(ty, sub_resource_props.get(id)),
                    ))
                }),
                _ => None,
            };
            if let Some((shape_id, shape_class, p)) = resolved {
                tmpl.properties
                    .insert("__viewport_shape__".into(), Variant::String(p));
                // The physics layer needs the shape's real geometry, not a gizmo
                // string: copy the sub-resource's class and its typed properties
                // onto the node under `__shape__*`, so
                // `physics_server::shape_from_node` can build a body shape with the
                // authored dimensions instead of falling back to a default circle
                // (pat-0q2kx.28). Copying every property verbatim keeps this free
                // of per-shape knowledge — `radius`, `size`, `a`/`b`, `points`,
                // `segments` all come along.
                tmpl.properties
                    .insert("__shape__class".into(), Variant::String(shape_class));
                if let Some(shape_props) = sub_resource_props.get(&shape_id) {
                    for (key, value) in shape_props {
                        tmpl.properties
                            .insert(format!("__shape__{key}"), value.clone());
                    }
                }
            }
        }

        // `LightOccluder2D.occluder` is an `OccluderPolygon2D` sub-resource
        // holding the actual geometry (`polygon`, `closed`, `cull_mode`) —
        // upstream/godot/scene/2d/light_occluder_2d.cpp. Flattened here for the
        // same reason as `__shape__*`: by render time the sub-resource is gone
        // and the lighting pass can only see node properties (pat-s3rrv.21).
        //
        // Without this, `collect_occluders_2d` looked for a `polygon` directly
        // on the node, found `Nil` for every Godot-authored scene, and returned
        // ZERO occluders — so 2D shadows never appeared from a real `.tscn`.
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("occluder") {
                Some(Variant::String(s)) => s
                    .strip_prefix("SubResource:")
                    .map(|id| id.to_string())
                    .filter(|id| sub_resources.contains_key(id)),
                _ => None,
            };
            if let Some(occ_id) = resolved {
                if let Some(occ_props) = sub_resource_props.get(&occ_id) {
                    for (key, value) in occ_props {
                        tmpl.properties
                            .insert(format!("__occluder__{key}"), value.clone());
                    }
                }
            }
        }

        // Flatten a Label's `label_settings` sub-resource onto the node
        // (pat-s3rrv.127).
        //
        // `LabelSettings` carries the font size, colour, outline and shadow a
        // Label draws with, and NOTHING consumed it — `control_render` read only
        // the theme, so an authored LabelSettings changed nothing on screen.
        //
        // ⚠️ The companion keys are prefixed rather than written bare: a Label
        // node has its own `theme_override_*` properties, and a bare `font_size`
        // on the node would collide with the theme-override lookup rather than
        // sit beside it.
        for tmpl in &mut templates {
            let Some(id) = sub_ref_id(tmpl.properties.get("label_settings")) else {
                continue;
            };
            let Some(props) = sub_resource_props.get(&id) else {
                continue;
            };
            // A marker, so the draw path can tell "settings present with default
            // values" from "no settings at all". 🔑 That distinction is
            // load-bearing: Godot's Label uses `has_settings ? settings->X :
            // theme->X` per field with NO per-field fallback, so a LabelSettings
            // that sets only `outline_size` still forces font_size to the
            // resource's own default and overrides the theme.
            tmpl.properties
                .insert("__label_settings__".into(), Variant::Bool(true));
            for (key, value) in props {
                tmpl.properties
                    .insert(format!("__ls__{key}"), value.clone());
            }
        }

        // Resolve `multimesh = SubResource("id")` on MultiMeshInstance3D by
        // copying the MultiMesh body onto the node under `__multimesh__*`, the
        // same way `__shape__*` above carries a CollisionShape's real geometry
        // (pat-s3rrv.112). Before this, every expanded instance read the node's
        // single `instance_transform` property, so a MultiMeshInstance3D drew N
        // copies stacked at one position.
        //
        // Copying every property verbatim keeps this free of per-field
        // knowledge: `buffer`, `instance_count`, `transform_format`,
        // `use_colors` and `use_custom_data` all come along, and the real
        // `multimesh` property keeps its `SubResource:<id>` value so the scene
        // still round-trips.
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("multimesh") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").and_then(|id| {
                    let ty = sub_resources.get(id)?;
                    Some((id.to_string(), ty.clone()))
                }),
                _ => None,
            };
            if let Some((mm_id, mm_class)) = resolved {
                tmpl.properties
                    .insert("__multimesh__class".into(), Variant::String(mm_class));
                if let Some(mm_props) = sub_resource_props.get(&mm_id) {
                    for (key, value) in mm_props {
                        tmpl.properties
                            .insert(format!("__multimesh__{key}"), value.clone());
                    }
                    // 🔑 The MultiMesh's own `mesh` is ITSELF a sub-resource
                    // reference, so the verbatim copy above leaves
                    // `__multimesh__mesh` as the bare string `SubResource:<id>`
                    // — an id with no class and no properties. Nothing
                    // downstream could turn that into geometry, which is why
                    // every authored MultiMesh drew the fallback cube however
                    // its mesh was authored.
                    //
                    // Recording the nested resource's CLASS alongside it mirrors
                    // `__multimesh__class` for the MultiMesh itself, and keeps
                    // this pass free of per-mesh-type knowledge — the class name
                    // is all `build_primitive` needs (pat-mgixk.50).
                    if let Some(Variant::String(mesh_ref)) = mm_props.get("mesh") {
                        if let Some(mesh_id) = mesh_ref.strip_prefix("SubResource:") {
                            if let Some(mesh_class) = sub_resources.get(mesh_id) {
                                tmpl.properties.insert(
                                    "__multimesh__mesh_class".into(),
                                    Variant::String(mesh_class.clone()),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Resolve `skin = SubResource("id")` on MeshInstance3D by copying the
        // Skin body onto the node under `__skin__*`, exactly as the MultiMesh
        // pass above does (pat-s3rrv.200).
        //
        // 🔑 A `SubResource:<id>` string names a class and an id and carries no
        // bind data at all, so before this the runtime had nothing to read even
        // in principle and `resolve_skin` fell back to the skeleton's rest pose
        // for every rigged model. That fallback still animates, which is why
        // the gap survived: it is wrong only where the authored bind poses
        // differ from the rest inverses.
        //
        // Copying every property verbatim keeps this free of per-field
        // knowledge — `bind_count` and each `bind/<i>/bone`, `bind/<i>/name`,
        // `bind/<i>/pose` come along — and the real `skin` property keeps its
        // `SubResource:<id>` value so the scene still round-trips.
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("skin") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").and_then(|id| {
                    let ty = sub_resources.get(id)?;
                    Some((id.to_string(), ty.clone()))
                }),
                _ => None,
            };
            if let Some((skin_id, skin_class)) = resolved {
                tmpl.properties
                    .insert("__skin__class".into(), Variant::String(skin_class));
                if let Some(skin_props) = sub_resource_props.get(&skin_id) {
                    for (key, value) in skin_props {
                        tmpl.properties
                            .insert(format!("__skin__{key}"), value.clone());
                    }
                }
            }
        }

        // Resolve a `texture = SubResource("id")` that names a **NoiseTexture2D**
        // by copying its body onto the node under `__noise_tex__*`, and the
        // FastNoiseLite it points at under `__noise__*` (pat-s3rrv.208).
        //
        // 🔑 This is a TWO-HOP resolution and that is the whole difficulty: the
        // Sprite2D names a NoiseTexture2D, whose own `noise` property names a
        // FastNoiseLite in a *separate* `[sub_resource]` section. Flattening
        // only the first hop yields a texture with no generator — a blank
        // image, which is exactly the "renders a placeholder" symptom the bead
        // describes.
        //
        // Unlike a CanvasTexture there is no file behind this: the pixels are
        // generated, so the render path bakes them rather than loading them.
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("texture") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").and_then(|id| {
                    let ty = sub_resources.get(id)?;
                    (ty == "NoiseTexture2D").then(|| id.to_string())
                }),
                _ => None,
            };
            let Some(tex_id) = resolved else { continue };
            tmpl.properties.insert(
                "__noise_tex__class".into(),
                Variant::String("NoiseTexture2D".into()),
            );
            let Some(tex_props) = sub_resource_props.get(&tex_id) else {
                continue;
            };
            for (key, value) in tex_props {
                tmpl.properties
                    .insert(format!("__noise_tex__{key}"), value.clone());
            }
            // Second hop: the generator itself.
            let noise_id = match tex_props.get("noise") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").map(|i| i.to_string()),
                _ => None,
            };
            let Some(noise_id) = noise_id else { continue };
            if let Some(class) = sub_resources.get(&noise_id) {
                tmpl.properties
                    .insert("__noise__class".into(), Variant::String(class.clone()));
            }
            if let Some(noise_props) = sub_resource_props.get(&noise_id) {
                for (key, value) in noise_props {
                    tmpl.properties
                        .insert(format!("__noise__{key}"), value.clone());
                }
            }
        }

        // Resolve a `texture = SubResource("id")` that names a **CanvasTexture**
        // by copying its body onto the node under `__canvas_texture__*`
        // (pat-s3rrv.203).
        //
        // ⚠️ Unlike the MultiMesh and Skin passes above, this one is CONDITIONAL
        // on the sub-resource's class. `texture` on a Sprite2D almost always
        // names an ImageTexture or AtlasTexture; stamping one of those under a
        // CanvasTexture prefix would tell the light pass that a normal map
        // exists when none does, and normal-mapped shading of a surface whose
        // "normals" are really albedo produces plausible-looking nonsense
        // rather than an error.
        //
        // A CanvasTexture's three channels are themselves resource references
        // (`diffuse_texture = ExtResource("1_x")`), so any `ExtResource:<id>`
        // value is resolved to its path on the way through: render time has the
        // tree but not the PackedScene, so an unresolved id is a dead end.
        // Everything else — `specular_color`, `specular_shininess`,
        // `texture_filter`, `texture_repeat` — is copied verbatim, which keeps
        // this free of per-field knowledge.
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("texture") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").and_then(|id| {
                    let ty = sub_resources.get(id)?;
                    (ty == "CanvasTexture").then(|| id.to_string())
                }),
                _ => None,
            };
            if let Some(ct_id) = resolved {
                tmpl.properties.insert(
                    "__canvas_texture__class".into(),
                    Variant::String("CanvasTexture".into()),
                );
                if let Some(ct_props) = sub_resource_props.get(&ct_id) {
                    for (key, value) in ct_props {
                        let resolved_value = match value {
                            Variant::String(s) => match s
                                .strip_prefix("ExtResource:")
                                .and_then(|ext_id| ext_resources.get(ext_id))
                            {
                                Some(entry) => Variant::String(entry.path.clone()),
                                None => value.clone(),
                            },
                            _ => value.clone(),
                        };
                        tmpl.properties
                            .insert(format!("__canvas_texture__{key}"), resolved_value);
                    }
                }
            }
        }

        // Resolve `texture_click_mask = SubResource("id")` on TextureButton to the
        // BitMap's `data` payload, copied onto the node as `__click_mask__data`
        // (pat-0q2kx.257).
        //
        // The real `texture_click_mask` property keeps its `SubResource:<id>`
        // string so the scene round-trips and the inspector still shows a
        // resource reference — the same split every companion above uses. The
        // copy is what makes the mask *reachable*: the `SubResource:<id>` string
        // carries no bits, so without it `control::texture_button_click_mask`
        // has nothing to sample and every click passes.
        //
        // A `.tscn` writes the payload as
        // `data = { "size": Vector2i(w, h), "data": PackedByteArray("<base64>") }`,
        // which `parse_variant_value` turns into a Dictionary with those exact
        // string keys — the shape `BitMap::from_data_dict` expects.
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("texture_click_mask") {
                Some(Variant::String(s)) => s.strip_prefix("SubResource:").and_then(|id| {
                    // Gated on the sub-resource's type: `texture_click_mask` is
                    // typed `BitMap` in Godot, and copying a non-BitMap's `data`
                    // here would hand the hit test a payload it would silently
                    // fail to decode.
                    if sub_resources.get(id).map(String::as_str) != Some("BitMap") {
                        return None;
                    }
                    sub_resource_props.get(id)?.get("data").cloned()
                }),
                _ => None,
            };
            if let Some(data) = resolved {
                tmpl.properties.insert("__click_mask__data".into(), data);
            }
        }

        // Resolve `navigation_polygon` / `navigation_mesh` = SubResource("id") on
        // NavigationRegion2D/3D to the resource's real geometry, stored under
        // companion properties so the declared property keeps its
        // `SubResource:<id>` value (round-trip + inspector) — same shape as the
        // `mesh` / `shape` resolution above.
        //
        // Godot's NavigationPolygon/NavigationMesh carry `vertices` **plus a
        // `polygons` array of index arrays** — several polygons per region, which
        // is what makes a region traversable across its interior
        // (navigation_region_2d.cpp `region_set_navigation_polygon`). Before this,
        // `MainLoop::process_navigation` read an invented `navmesh_vertices`
        // property that no loader ever produced, so a .tscn-authored region
        // contributed nothing to the navigation map (pat-0q2kx.26).
        for tmpl in &mut templates {
            let sub_id =
                ["navigation_polygon", "navigation_mesh"]
                    .iter()
                    .find_map(|key| match tmpl.properties.get(*key) {
                        Some(Variant::String(s)) => {
                            s.strip_prefix("SubResource:").map(String::from)
                        }
                        _ => None,
                    });
            let Some(sub_id) = sub_id else {
                continue;
            };
            let Some(props) = sub_resource_props.get(&sub_id) else {
                continue;
            };
            // `vertices` is a PackedVector2Array (2D) or PackedVector3Array (3D);
            // the region's class decides how it is read back.
            if let Some(vertices) = props.get("vertices") {
                tmpl.properties
                    .insert("__navmesh_vertices__".into(), vertices.clone());
            }
            if let Some(polygons) = props.get("polygons") {
                tmpl.properties
                    .insert("__navmesh_polygons__".into(), polygons.clone());
            }
            // The BAKE parameters (pat-s3rrv.180). `vertices`/`polygons` above
            // are the baked *result*; these are the inputs a rebake needs, and
            // they live on the same resource.
            //
            // 🔑 Without this the whole parameter set stops at the resource
            // boundary. `gdserver3d::bake_navigation_mesh` reads
            // `agent_max_slope` correctly and always saw the hardcoded default,
            // so a scene that chose 60 degrees baked as though it had chosen 45
            // — the reader was wired and the value never arrived.
            for name in [
                "cell_size",
                "cell_height",
                "agent_height",
                "agent_radius",
                "agent_max_climb",
                "agent_max_slope",
                "region_min_size",
                "edge_max_length",
            ] {
                if let Some(value) = props.get(name) {
                    tmpl.properties.insert(
                        format!("{}{name}__", crate::nav_bake::PARAM_PREFIX),
                        value.clone(),
                    );
                }
            }
        }

        // pat-s3rrv.185: a `Skeleton2D`'s modification stack, two sub-resource
        // levels down. Same reason as the navmesh params above — the data is in
        // the file and stops at the resource boundary.
        for tmpl in &mut templates {
            let Some(Variant::String(reference)) = tmpl.properties.get("modification_stack") else {
                continue;
            };
            let Some(stack_id) = reference.strip_prefix("SubResource:").map(String::from) else {
                continue;
            };
            let mut flat: HashMap<String, Variant> = HashMap::new();
            flatten_modification_stack(&sub_resources, &sub_resource_props, &stack_id, &mut flat);
            tmpl.properties.extend(flat);
        }

        // Resolve `tile_set = SubResource("id")` on TileMapLayer/TileMap nodes
        // (pat-0q2kx.108). Unlike the passes above this is a *two-level* walk:
        // the TileSet resource lists `sources/<id> = SubResource("...")`, and it
        // is those `TileSetAtlasSource` sub-resources that carry the texture and
        // `texture_region_size` a cell's atlas coordinates index into. Both
        // levels are flattened onto the node under `__tile_set__*` so the
        // renderer can resolve a cell to a texture region from the tree alone,
        // exactly as `__shape__*` lets the physics server build a body shape.
        // The declared `tile_set` property keeps its `SubResource:<id>` value.
        for tmpl in &mut templates {
            // Owned: the inserts below take `tmpl.properties` mutably, so the
            // reference cannot stay borrowed out of it.
            let raw = match tmpl.properties.get("tile_set") {
                Some(Variant::String(raw)) => raw.clone(),
                _ => continue,
            };
            if let Some(set_id) = raw.strip_prefix("SubResource:") {
                let Some(set_props) = sub_resource_props.get(set_id) else {
                    continue;
                };
                let set_props = set_props.clone();
                flatten_tile_set_into(
                    &mut tmpl.properties,
                    &set_props,
                    &sub_resource_props,
                    &ext_resources,
                );
            } else if let Some(path) = parse_ext_resource_ref(&raw)
                .and_then(|id| ext_resources.get(&id))
                .map(|e| e.path.clone())
            {
                // `tile_set = ExtResource("1")` — a TileSet saved as its own
                // `.tres`, which is how real projects keep one (the official 2D
                // Platformer demo does). Only the SubResource form used to be
                // flattened, so such a TileMapLayer drew nothing and collided
                // with nothing: the player fell through the level forever.
                // The `.tres` is read with this same parser, so its
                // `sources/<id>` sub-resources and textures resolve against
                // ITS OWN tables, exactly as an inline tile set's do.
                if let Some(tres) = read_tile_set_tres(&path) {
                    flatten_tile_set_into(
                        &mut tmpl.properties,
                        &tres.set_props,
                        &tres.sub_resource_props,
                        &tres.ext_resources,
                    );
                }
            }
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
            sub_resources,
            sub_resource_props,
            editable_instances,
        })
    }

    /// Returns the class name of the inline sub-resource with `id`, if any
    /// (e.g. `"Environment"`, `"Sky"`, `"ProceduralSkyMaterial"`).
    pub fn sub_resource_type(&self, id: &str) -> Option<&str> {
        self.sub_resources.get(id).map(String::as_str)
    }

    /// Returns the parsed properties of the inline sub-resource with `id`, if
    /// any. Used to resolve a node's `SubResource(...)` reference to concrete
    /// resource fields.
    pub fn sub_resource_props(&self, id: &str) -> Option<&HashMap<String, Variant>> {
        self.sub_resource_props.get(id)
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
        let mut root_node = template_node(root_tmpl);
        apply_template_properties(&mut root_node, root_tmpl);
        for group in &root_tmpl.groups {
            root_node.add_to_group(group.clone());
        }
        root_node.set_unique_name(root_tmpl.unique_name);
        if let Some(ref inst) = root_tmpl.instance {
            root_node.set_property("_instance", Variant::String(inst.clone()));
            // Preserve the resolved sub-scene path so the saver can rebuild the
            // `[ext_resource type="PackedScene" ...]` entry (pat-nzgdu.18).
            if let Some(path) = self.resolve_ext_resource_path(inst) {
                root_node.set_property("_instance_path", Variant::String(path.to_string()));
            }
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

            let mut node = template_node(tmpl);
            apply_template_properties(&mut node, tmpl);
            for group in &tmpl.groups {
                node.add_to_group(group.clone());
            }
            node.set_unique_name(tmpl.unique_name);
            if let Some(ref inst) = tmpl.instance {
                node.set_property("_instance", Variant::String(inst.clone()));
                // Preserve the resolved sub-scene path (pat-nzgdu.18).
                if let Some(path) = self.resolve_ext_resource_path(inst) {
                    node.set_property("_instance_path", Variant::String(path.to_string()));
                }
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

    /// Node paths declared editable via `[editable path="..."]` — sub-scene
    /// instances whose local property/child overrides apply on load. Empty when
    /// none are present.
    pub fn editable_instances(&self) -> &[String] {
        &self.editable_instances
    }

    /// Nodes that defer instancing via `instance_placeholder`, as
    /// `(node_name, placeholder_res_path)` pairs.
    pub fn placeholder_nodes(&self) -> Vec<(String, String)> {
        self.templates
            .iter()
            .filter_map(|t| {
                t.instance_placeholder
                    .as_ref()
                    .map(|p| (t.name.clone(), p.clone()))
            })
            .collect()
    }

    /// Returns the external resources map (id -> entry).
    pub fn ext_resources(&self) -> &HashMap<String, ExtResourceEntry> {
        &self.ext_resources
    }

    /// Resolves an `ExtResource("id")` string to the resource path, if found.
    pub fn resolve_ext_resource_path(&self, ext_ref: &str) -> Option<&str> {
        // ext_ref is like `ExtResource("1_abc")`
        let id = parse_ext_resource_ref(ext_ref)?;
        self.ext_resources.get(&id).map(|e| e.path.as_str())
    }

    /// Instantiates this packed scene with recursive sub-scene resolution.
    ///
    /// When a node has `instance=ExtResource("id")`, the `resolve_scene`
    /// callback is invoked with the resolved `res://` path. If it returns
    /// `Some(PackedScene)`, that scene is instanced and inserted as a
    /// subtree, with property overrides from the parent scene applied on top.
    ///
    /// This enables full nested sub-scene instancing.
    pub fn instance_with_subscenes<F>(&self, resolve_scene: &F) -> EngineResult<Vec<Node>>
    where
        F: Fn(&str) -> Option<PackedScene>,
    {
        let mut levels = Vec::new();
        let (mut nodes, own) = self.instance_levels(resolve_scene, &mut levels)?;
        apply_level_owners(&mut nodes, &levels, &own);
        Ok(nodes)
    }

    /// [`instance_with_subscenes`](Self::instance_with_subscenes), also
    /// reporting every sub-scene LEVEL it flattened into the node list.
    ///
    /// Returns the nodes plus the indices THIS scene's own templates created;
    /// each instanced (or inherited) sub-scene is pushed onto `levels`, inner
    /// levels before outer ones, with its indices into the returned list.
    ///
    /// 🔑 The flat list alone loses which scene each node came from, and with
    /// it everything a scene carries besides nodes: its `[connection]`s (paths
    /// are relative to THAT scene's root) and its scene-local `SubResource`
    /// ids. Without the levels, a sub-scene's own signal wiring — a Player's
    /// `body_entered`, a HUD button's `pressed` — was silently dropped whenever
    /// a scene reached it through `instance=`.
    fn instance_levels<F>(
        &self,
        resolve_scene: &F,
        levels: &mut Vec<SceneLevel>,
    ) -> EngineResult<(Vec<Node>, Vec<usize>)>
    where
        F: Fn(&str) -> Option<PackedScene>,
    {
        let mut own: Vec<usize> = Vec::new();
        if self.templates.is_empty() {
            return Err(EngineError::InvalidOperation(
                "packed scene has no nodes".into(),
            ));
        }

        let root_tmpl = &self.templates[0];
        if root_tmpl.parent_path.is_some() {
            return Err(EngineError::Parse(
                "first [node] section must be the scene root (no parent attribute)".into(),
            ));
        }

        let mut nodes: Vec<Node> = Vec::new();
        let mut path_to_index: HashMap<String, usize> = HashMap::new();

        // An INHERITED scene: the root `[node]` carries `instance=` and no
        // `type=` (packed_scene.cpp SceneState). The base scene instantiates
        // first and this scene's templates apply as a diff on top.
        //
        // This used to be skipped with a comment saying the caller handled it,
        // which nothing did — so an inherited scene lost its entire base tree
        // and loaded as a bare root (pat-0q2kx.139). Recursive, so a scene
        // inheriting an inherited scene resolves all the way down.
        let inherited_base = root_tmpl
            .instance
            .as_ref()
            .and_then(|inst| self.resolve_ext_resource_path(inst))
            // Called through an explicit closure, not passed as `&F` directly:
            // `and_then` wants an owned `FnOnce(&str)`, and handing it the
            // borrowed generic callback makes trait resolution fight the
            // higher-ranked lifetime on `F: Fn(&str)`. Every other use of
            // `resolve_scene` in this file is either a direct call or a `&F`
            // parameter, which is why this was the one that broke.
            .and_then(|path| resolve_scene(path));

        if let Some(base) = inherited_base {
            let mut base_levels = Vec::new();
            let (base_nodes, base_own) = base.instance_levels(resolve_scene, &mut base_levels)?;
            levels.extend(base_levels);
            // The base's nodes come first, so its indices need no offset.
            levels.push(SceneLevel {
                scene: base,
                root: 0,
                own: base_own,
            });
            if base_nodes.is_empty() {
                return Err(EngineError::InvalidOperation(
                    "inherited base scene produced no nodes".into(),
                ));
            }
            nodes = base_nodes;
            // The derived scene names the root and diffs its properties on top.
            nodes[0].set_name(&root_tmpl.name);
            apply_template_properties(&mut nodes[0], root_tmpl);
            for group in &root_tmpl.groups {
                nodes[0].add_to_group(group.clone());
            }
            nodes[0].set_unique_name(root_tmpl.unique_name);
            if let Some(ref script_path) = root_tmpl.script_path {
                nodes[0].set_property("_script_path", Variant::String(script_path.clone()));
            }
            path_to_index.insert(".".into(), 0);
            path_to_index.insert(root_tmpl.name.clone(), 0);
            // Base children are addressable by the bare paths the derived
            // scene's templates use (`Sprite`, `Sprite/Child`), so a diff entry
            // finds them instead of creating a duplicate.
            Self::map_root_child_paths(&nodes, &mut path_to_index);
        } else {
            let mut root_node = template_node(root_tmpl);
            apply_template_properties(&mut root_node, root_tmpl);
            for group in &root_tmpl.groups {
                root_node.add_to_group(group.clone());
            }
            root_node.set_unique_name(root_tmpl.unique_name);
            if let Some(ref script_path) = root_tmpl.script_path {
                root_node.set_property("_script_path", Variant::String(script_path.clone()));
            }
            path_to_index.insert(".".into(), 0);
            path_to_index.insert(root_tmpl.name.clone(), 0);
            nodes.push(root_node);
            own.push(0);
        }
        let root_id = nodes[0].id();

        for tmpl in &self.templates[1..] {
            let parent_path = tmpl.parent_path.as_deref().unwrap_or(".");
            let parent_idx = path_to_index.get(parent_path).copied().ok_or_else(|| {
                EngineError::Parse(format!(
                    "parent path '{parent_path}' not found for node '{}'",
                    tmpl.name
                ))
            })?;

            // Check if this node instances a sub-scene.
            if let Some(ref inst_ref) = tmpl.instance {
                if let Some(scene_path) = self.resolve_ext_resource_path(inst_ref) {
                    if let Some(sub_scene) = resolve_scene(scene_path) {
                        // Recursively instance the sub-scene.
                        let mut sub_levels = Vec::new();
                        let (sub_nodes, sub_own) =
                            sub_scene.instance_levels(resolve_scene, &mut sub_levels)?;
                        if sub_nodes.is_empty() {
                            continue;
                        }

                        // The sub-scene root replaces this template's node.
                        // Apply overrides: rename to template name, apply
                        // template properties on top.
                        let sub_root_idx = nodes.len();
                        levels.extend(
                            sub_levels
                                .into_iter()
                                .map(|level| level.offset_by(sub_root_idx)),
                        );
                        levels.push(
                            SceneLevel {
                                scene: sub_scene,
                                root: 0,
                                own: sub_own,
                            }
                            .offset_by(sub_root_idx),
                        );
                        let node_path = if parent_path == "." {
                            tmpl.name.clone()
                        } else {
                            format!("{}/{}", parent_path, tmpl.name)
                        };

                        for (i, mut sub_node) in sub_nodes.into_iter().enumerate() {
                            if i == 0 {
                                // Override name from parent scene's template.
                                sub_node.set_name(&tmpl.name);
                                // Apply property overrides from parent scene.
                                apply_template_properties(&mut sub_node, tmpl);
                                for group in &tmpl.groups {
                                    sub_node.add_to_group(group.clone());
                                }
                                sub_node.set_unique_name(tmpl.unique_name);
                                // Wire to parent.
                                sub_node.set_parent(Some(nodes[parent_idx].id()));
                                sub_node.set_owner(Some(root_id));
                                let child_id = sub_node.id();
                                nodes[parent_idx].add_child_id(child_id);
                                path_to_index.insert(node_path.clone(), sub_root_idx);
                            } else {
                                // Remap sub-node owners to the main scene root.
                                sub_node.set_owner(Some(root_id));
                            }
                            nodes.push(sub_node);
                        }

                        // Map sub-scene child paths for future parent lookups.
                        // Recursively map children of the sub-root.
                        Self::map_child_paths(&nodes, sub_root_idx, &node_path, &mut path_to_index);

                        continue;
                    }
                }
            }

            let node_path = if parent_path == "." {
                tmpl.name.clone()
            } else {
                format!("{}/{}", parent_path, tmpl.name)
            };

            // An OVERRIDE entry: a `[node]` whose path already exists, because
            // it sits inside an instanced sub-scene or an inherited base. Godot
            // writes every tweaked instance this way — `[node name="Sprite"
            // parent="Enemy"]` with no `type=` — and it must *modify* the
            // existing node (packed_scene.cpp SceneState).
            //
            // This branch used to be absent: the code below unconditionally
            // built a new node, so an override produced a DUPLICATE sibling and
            // the real instanced child kept its base values (pat-0q2kx.139).
            if let Some(&existing_idx) = path_to_index.get(&node_path) {
                let existing = &mut nodes[existing_idx];
                apply_template_properties(existing, tmpl);
                for group in &tmpl.groups {
                    existing.add_to_group(group.clone());
                }
                // Only ever turn uniqueness on: an override that omits the `%`
                // marker is not asking to revoke it.
                if tmpl.unique_name {
                    existing.set_unique_name(true);
                }
                if let Some(ref script_path) = tmpl.script_path {
                    existing.set_property("_script_path", Variant::String(script_path.clone()));
                }
                continue;
            }

            // A deferred sub-scene: `instance_placeholder="res://…"` stands in
            // for the real scene until `create_instance()` is called. Godot
            // gives these the `InstancePlaceholder` class and stores the path
            // plus the authored property bag for `get_stored_values()`.
            // Previously they loaded as plain empty `Node`s, losing both.
            if let Some(ref placeholder_path) = tmpl.instance_placeholder {
                let mut node = Node::new(&tmpl.name, "InstancePlaceholder");
                node.set_property(
                    "_instance_placeholder_path",
                    Variant::String(placeholder_path.clone()),
                );
                // The authored properties are the "stored values" the
                // placeholder hands back — they are NOT applied to the
                // placeholder itself, since it stands in for a different scene.
                // Sorted by key: `properties` is a `HashMap`, so emitting it in
                // iteration order would make `get_stored_values()` return a
                // different order run to run.
                let mut keys: Vec<&String> = tmpl.properties.keys().collect();
                keys.sort();
                let stored: Vec<Variant> = keys
                    .into_iter()
                    .flat_map(|k| [Variant::String(k.clone()), tmpl.properties[k].clone()])
                    .collect();
                node.set_property("_stored_values", Variant::Array(stored.into()));
                for group in &tmpl.groups {
                    node.add_to_group(group.clone());
                }
                node.set_unique_name(tmpl.unique_name);
                node.set_owner(Some(root_id));
                let child_id = node.id();
                path_to_index.insert(node_path, nodes.len());
                node.set_parent(Some(nodes[parent_idx].id()));
                nodes[parent_idx].add_child_id(child_id);
                own.push(nodes.len());
                nodes.push(node);
                continue;
            }

            // Regular (non-instanced) node.
            let mut node = template_node(tmpl);
            apply_template_properties(&mut node, tmpl);
            for group in &tmpl.groups {
                node.add_to_group(group.clone());
            }
            node.set_unique_name(tmpl.unique_name);
            if let Some(ref inst) = tmpl.instance {
                node.set_property("_instance", Variant::String(inst.clone()));
                // Preserve the resolved sub-scene path (pat-nzgdu.18).
                if let Some(path) = self.resolve_ext_resource_path(inst) {
                    node.set_property("_instance_path", Variant::String(path.to_string()));
                }
            }
            if let Some(ref script_path) = tmpl.script_path {
                node.set_property("_script_path", Variant::String(script_path.clone()));
            }
            node.set_owner(Some(root_id));

            let child_id = node.id();
            let child_idx = nodes.len();
            path_to_index.insert(node_path, child_idx);

            node.set_parent(Some(nodes[parent_idx].id()));
            nodes[parent_idx].add_child_id(child_id);

            own.push(child_idx);
            nodes.push(node);
        }

        Ok((nodes, own))
    }

    /// Maps every descendant of node 0 by the *bare* path a template uses.
    ///
    /// [`map_child_paths`](Self::map_child_paths) prefixes with its `root_path`,
    /// which would give `./Sprite` for a root child — but a template with
    /// `parent="."` addresses it as plain `Sprite`, so an inherited scene's diff
    /// entries would miss and duplicate every node (pat-0q2kx.139).
    fn map_root_child_paths(nodes: &[Node], path_to_index: &mut HashMap<String, usize>) {
        let root_id = nodes[0].id();
        for (idx, node) in nodes.iter().enumerate() {
            if idx == 0 || node.parent() != Some(root_id) {
                continue;
            }
            let child_path = node.name().to_string();
            path_to_index.insert(child_path.clone(), idx);
            Self::map_child_paths(nodes, idx, &child_path, path_to_index);
        }
    }

    /// Recursively maps child node paths for a subtree starting at `root_idx`.
    fn map_child_paths(
        nodes: &[Node],
        root_idx: usize,
        root_path: &str,
        path_to_index: &mut HashMap<String, usize>,
    ) {
        let root_id = nodes[root_idx].id();
        for (idx, node) in nodes.iter().enumerate() {
            if idx == root_idx {
                continue;
            }
            if node.parent() == Some(root_id) {
                let child_path = format!("{}/{}", root_path, node.name());
                path_to_index.insert(child_path.clone(), idx);
                Self::map_child_paths(nodes, idx, &child_path, path_to_index);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The node a template starts from before its authored properties apply:
/// ClassDB-seeded for a typed `[node]`, BARE for a typeless one (an `instance=`
/// node or an override entry), whose real class lives in another file — see
/// [`NodeTemplate::has_type`].
fn template_node(tmpl: &NodeTemplate) -> Node {
    if tmpl.has_type {
        node_with_class_defaults(&tmpl.name, &tmpl.class_name)
    } else {
        Node::new(&tmpl.name, &tmpl.class_name)
    }
}

/// Applies a template's authored properties over its (seeded) node.
///
/// 🔑 A `.tscn` 3D node stores only `transform = Transform3D(..)`; Godot's
/// `position`/`rotation`/`scale` are views of that one `local_transform`
/// (`scene/3d/node_3d.cpp`). Patina keeps them as separate keys, and ClassDB
/// seeding writes `position = ZERO` etc. BEFORE the authored values — so a node
/// authored at (5,6,7) answered `get_position()` with ZERO while
/// `get_local_transform()` said (5,6,7). When the template authors the
/// transform and none of its component views, every view is refreshed from it.
fn apply_template_properties(node: &mut Node, tmpl: &NodeTemplate) {
    for (key, value) in &tmpl.properties {
        node.set_property(key, instance_value(value));
    }
    const VIEWS: &[&str] = &["position", "rotation", "rotation_degrees", "scale", "basis", "quaternion"];
    let authored_view = tmpl.properties.iter().any(|(k, _)| VIEWS.contains(&k.as_str()));
    if authored_view {
        return;
    }
    if let Variant::Transform3D(t) = node.get_property("transform") {
        if tmpl.properties.iter().any(|(k, _)| k == "transform") {
            crate::node3d::store_local_transform(node, t);
        }
    }
}

/// Builds a scene node the way Godot instantiates one: through ClassDB, so the
/// class's registered defaults are already in the property bag before the
/// scene's stored values override a subset of them (pat-0q2kx.210).
///
/// Without this, a member the `.tscn` omits read back as `Nil` instead of its
/// class default — `scale` was `Nil` rather than `(1, 1)` on every node loaded
/// from a scene file, while a node made by `SceneTree::create_node` had it.
///
/// Deliberately the same walk as [`crate::SceneTree::create_node`] so the two
/// build paths cannot drift: `get_property_list` returns the inheritance chain
/// base→derived without deduping, so a derived class's own default overwrites
/// its base's — the order Godot applies them in. An unpopulated ClassDB (no
/// `register_*_classes()` call yet) yields an empty list and seeds nothing, so
/// this is inert rather than wrong in bare tests.
fn node_with_class_defaults(name: &str, class_name: &str) -> Node {
    let mut node = Node::new(name, class_name);
    let props = gdobject::class_db::get_property_list(class_name);
    // 🔑 A Node3D's `transform` is a DERIVED VIEW of position/rotation/scale,
    // not storage of its own, so seeding its identity default is actively
    // harmful: `node3d::local_transform_of` PREFERS an explicit `transform`,
    // and a seeded one is indistinguishable from a `.tscn`-authored one. The
    // defaults are applied BEFORE the scene's own properties, so a node
    // authoring bare `position = Vector3(10, 0, 0)` ended up with an identity
    // `transform` beating it — and rendered at the ORIGIN (pat-mgixk.29).
    //
    // Godot has no such conflict: `position`/`rotation`/`scale`/`transform` are
    // all views of one `data.local_transform` (`scene/3d/node_3d.cpp`), so
    // there is nothing to prefer. Patina stores them separately, so precedence
    // decides, and a DEFAULT must never beat an AUTHORED value.
    //
    // ⚠️ Scoped two ways to keep the blast radius minimal. Only a
    // `Transform3D`-valued default is skipped, so Node2D's `transform`
    // (Transform2D) is untouched — nothing treats that one as canonical. And
    // only when the class also registers `position`, so a class whose
    // `transform` really is its own storage keeps its default.
    //
    // Nothing is lost by not seeding it: an unauthored node composes identity
    // from position ZERO / rotation ZERO / scale ONE, and every setter writes
    // `transform` back through `node3d::store_local_transform`.
    let derives_transform = props.iter().any(|p| p.name == "position");
    for prop in props {
        if derives_transform
            && prop.name == "transform"
            && matches!(prop.default_value, Variant::Transform3D(_))
        {
            continue;
        }
        node.set_property(&prop.name, prop.default_value);
    }
    node
}

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
/// Encodes a collision shape sub-resource as a `shape:<Type>[|dims]` marker for
/// the 3D viewport's wireframe gizmo. `dims` carries the shape's real extents
/// (box `size`, sphere `radius`, capsule/cylinder `radius,height`) when present,
/// so the gizmo matches the actual collision bounds rather than a default size.
/// Flattens a `TileSetAtlasSource`'s per-tile collision polygons onto the
/// TileMap node's properties (pat-0q2kx.117).
///
/// Godot writes one key per tile, per physics layer, per polygon:
/// `"<ax>:<ay>/<alt>/physics_layer_<n>/polygon_<m>/points"` and a sibling
/// `".../one_way"`. Only the first polygon of the first physics layer is
/// carried across — [`Shape2D::ConvexPolygon`](gdphysics2d::shape::Shape2D)
/// is a single convex hull, so a multi-polygon tile would need several bodies
/// per cell, which the tile physics path does not model yet.
///
/// Returns whether the source contributed any collision at all, so the caller
/// can record it in `__tile_physics_sources__`.
/// Flattens a `TileSetAtlasSource`'s navigation polygons onto the node
/// (pat-s3rrv.65). Returns `true` when the source has any.
///
/// Mirrors [`collect_tile_physics`], with one difference that is not cosmetic:
///
/// ⚠️ **The two families do not share an origin.** Godot authors both relative
/// to the tile CENTRE, and physics carries over untouched because
/// `tilemap_to_physics` places the body at that centre. Navigation does not:
/// [`TILE_NAV_POLYGON`](crate::tilemap::TILE_NAV_POLYGON) is tile-local from the
/// TOP-LEFT corner, and `tile_navigation_polygons` adds the cell corner to every
/// point. So half a tile goes back ON here, exactly as the saver takes it off.
/// Skip the rebasing and the walkable area lands over the tile's top-left
/// corner — a quarter-tile of overlap per cell, which still produces a
/// navigable-looking mesh.
///
/// ⚠️ Patina's model holds ONE polygon for the whole layer, where Godot holds
/// one per `TileData`. The first tile in atlas order wins and per-tile variation
/// is lost; a file that varies its polygon per tile round-trips to the first
/// one. Widening that is a model change in `tilemap.rs`, not a parsing change.
/// Flattens a `TileSetAtlasSource`'s occluder polygons onto the node
/// (pat-s3rrv.65). Returns `true` when the source has any.
///
/// ⚠️ TWO key spellings, and Godot writes the LONGER one:
/// `occlusion_layer_<i>/polygon_<j>/polygon` is what a real save contains, while
/// the two-component `occlusion_layer_<i>/polygon` is explicitly "Kept for
/// compatibility" (`tile_set.cpp:6715`). A tile may hold SEVERAL occluder
/// polygons per layer, which is why the index exists at all — navigation has no
/// equivalent. Both are accepted here; only the indexed form is written.
///
/// The value is a `Ref<OccluderPolygon2D>` either way, so the geometry lives in
/// a sub-resource under `polygon` and half a tile goes back on, exactly as for
/// navigation.
/// Flattens one atlas source's per-tile custom data onto the node
/// (pat-s3rrv.65).
///
/// ⚠️ This whole path was MISSING, and silently: the editor authors
/// `__tile_custom_data_*` companions, `is_internal_prop` correctly keeps those
/// caches out of the saved file, and — unlike the physics, navigation and
/// occlusion families — nothing regenerated them as real Godot keys. A user set
/// per-tile custom data, saved, reloaded, and it was gone; a `.tscn` authored in
/// real Godot lost it on load.
///
/// ⚠️ The two key shapes are NOT the same prefix, which is the easy mistake:
/// the TileSet declares `custom_data_layer_<i>/name` (with a `/` component)
/// while a tile stores `<x>:<y>/<alt>/custom_data_<i>` (no `layer` word, no
/// sub-component). See `tile_set.cpp:4211` and `:6848`.
///
/// Values are copied VERBATIM. Godot types the layer separately
/// (`custom_data_layer_<i>/type`), so re-encoding here would be a second, worse
/// source of truth than the value's own Variant.
fn collect_tile_custom_data(
    source_id: i32,
    source_props: &HashMap<String, Variant>,
    layer_names: &[String],
    out: &mut HashMap<String, Variant>,
) {
    if layer_names.is_empty() {
        return;
    }
    for (key, value) in source_props {
        let Some((coords, rest)) = key.split_once('/') else {
            continue;
        };
        let Some((ax, ay)) = coords.split_once(':') else {
            continue;
        };
        let (Ok(ax), Ok(ay)) = (ax.trim().parse::<i32>(), ay.trim().parse::<i32>()) else {
            continue;
        };
        let Some((_alt, layer_rest)) = rest.split_once('/') else {
            continue;
        };
        // Exactly one component: `custom_data_<i>`. A key with a further `/`
        // belongs to another family and is not ours to read.
        let Some(index) = layer_rest.strip_prefix("custom_data_") else {
            continue;
        };
        let Ok(index) = index.parse::<usize>() else {
            continue;
        };
        let Some(name) = layer_names.get(index) else {
            continue;
        };
        out.insert(
            crate::tilemap::tile_custom_data_key(
                source_id,
                gdcore::math::Vector2i::new(ax, ay),
                name,
            ),
            value.clone(),
        );
    }
}

fn collect_tile_occlusion(
    source_id: i32,
    source_props: &HashMap<String, Variant>,
    sub_resource_props: &HashMap<String, HashMap<String, Variant>>,
    tile_size: gdcore::math::Vector2i,
    out: &mut HashMap<String, Variant>,
) -> bool {
    let half = gdcore::math::Vector2::new(tile_size.x as f32 / 2.0, tile_size.y as f32 / 2.0);
    let mut found: Vec<(gdcore::math::Vector2i, Vec<gdcore::math::Vector2>)> = Vec::new();

    for (key, value) in source_props {
        let Some((coords, rest)) = key.split_once('/') else {
            continue;
        };
        let Some((ax, ay)) = coords.split_once(':') else {
            continue;
        };
        let (Ok(ax), Ok(ay)) = (ax.trim().parse::<i32>(), ay.trim().parse::<i32>()) else {
            continue;
        };
        let Some((_alt, layer_rest)) = rest.split_once('/') else {
            continue;
        };
        if !layer_rest.starts_with("occlusion_layer_") || !layer_rest.ends_with("/polygon") {
            continue;
        }
        let Variant::String(reference) = value else {
            continue;
        };
        let Some(id) = reference.strip_prefix("SubResource:") else {
            continue;
        };
        let Some(props) = sub_resource_props.get(id) else {
            continue;
        };
        let points = match props.get("polygon") {
            Some(Variant::PackedVector2Array(p)) => p.clone(),
            _ => continue,
        };
        if points.len() >= 3 {
            found.push((gdcore::math::Vector2i::new(ax, ay), points));
        }
    }

    if found.is_empty() {
        return false;
    }
    found.sort_by_key(|(a, _)| (a.y, a.x));

    let rebased: Vec<gdcore::math::Vector2> = found[0].1.iter().map(|p| *p + half).collect();
    let whole_tile = vec![
        gdcore::math::Vector2::new(0.0, 0.0),
        gdcore::math::Vector2::new(tile_size.x as f32, 0.0),
        gdcore::math::Vector2::new(tile_size.x as f32, tile_size.y as f32),
        gdcore::math::Vector2::new(0.0, tile_size.y as f32),
    ];
    if rebased != whole_tile {
        out.insert(
            crate::tilemap::TILE_OCCLUDER_POLYGON.to_string(),
            Variant::PackedVector2Array(rebased),
        );
    }
    true
}

fn collect_tile_navigation(
    source_id: i32,
    source_props: &HashMap<String, Variant>,
    sub_resource_props: &HashMap<String, HashMap<String, Variant>>,
    tile_size: gdcore::math::Vector2i,
    out: &mut HashMap<String, Variant>,
) -> bool {
    let half = gdcore::math::Vector2::new(tile_size.x as f32 / 2.0, tile_size.y as f32 / 2.0);
    // Sorted so "the first tile" is the same tile on every load; a HashMap walk
    // would pick a different polygon run to run.
    let mut found: Vec<(gdcore::math::Vector2i, Vec<gdcore::math::Vector2>)> = Vec::new();

    for (key, value) in source_props {
        // Split `<ax>:<ay>/<alt>/navigation_layer_<n>/polygon`.
        let Some((coords, rest)) = key.split_once('/') else {
            continue;
        };
        let Some((ax, ay)) = coords.split_once(':') else {
            continue;
        };
        let (Ok(ax), Ok(ay)) = (ax.trim().parse::<i32>(), ay.trim().parse::<i32>()) else {
            continue;
        };
        let Some((_alt, layer_rest)) = rest.split_once('/') else {
            continue;
        };
        if !layer_rest.starts_with("navigation_layer_") || !layer_rest.ends_with("/polygon") {
            continue;
        }
        let points = match value {
            // ⚠️ The real Godot shape: the key holds a `NavigationPolygon`
            // RESOURCE, so the value is a SubResource reference and the geometry
            // lives in that section's `vertices`. Reading only the inline array
            // below would silently skip every polygon a real Godot project — or
            // this engine's own saver — writes (pat-s3rrv.65).
            Variant::String(reference) => {
                let Some(id) = reference.strip_prefix("SubResource:") else {
                    continue;
                };
                let Some(props) = sub_resource_props.get(id) else {
                    continue;
                };
                match props.get("vertices") {
                    Some(Variant::PackedVector2Array(p)) => p.clone(),
                    _ => continue,
                }
            }
            // Tolerated for hand-written scenes and older files: a bare point
            // array. Godot itself drops these (a PackedVector2Array converts to
            // a null `Ref<NavigationPolygon>`), so accepting it here is strictly
            // more forgiving than the engine we are matching.
            Variant::PackedVector2Array(p) => p.clone(),
            Variant::Array(items) => items
                .to_vec()
                .iter()
                .filter_map(|v| match v {
                    Variant::Vector2(p) => Some(*p),
                    _ => None,
                })
                .collect(),
            _ => continue,
        };
        if points.len() >= 3 {
            found.push((gdcore::math::Vector2i::new(ax, ay), points));
        }
    }

    if found.is_empty() {
        return false;
    }
    found.sort_by_key(|(a, _)| (a.y, a.x));

    let rebased: Vec<gdcore::math::Vector2> = found[0].1.iter().map(|p| *p + half).collect();
    // The whole-tile rect is the default; storing it explicitly would be
    // indistinguishable in behaviour but would make an unedited tile look
    // edited on the next save.
    let whole_tile = vec![
        gdcore::math::Vector2::new(0.0, 0.0),
        gdcore::math::Vector2::new(tile_size.x as f32, 0.0),
        gdcore::math::Vector2::new(tile_size.x as f32, tile_size.y as f32),
        gdcore::math::Vector2::new(0.0, tile_size.y as f32),
    ];
    if rebased != whole_tile {
        out.insert(
            crate::tilemap::TILE_NAV_POLYGON.to_string(),
            Variant::PackedVector2Array(rebased),
        );
    }
    true
}

/// Flattens each ALTERNATIVE tile's orientation — `<ax>:<ay>/<alt>/flip_h`,
/// `flip_v`, `transpose` — into one bit mask per alternative
/// ([`crate::tilemap::tile_alternative_flags_key`]). Godot draws a cell
/// painted with alternative N using that tile's flags
/// (`TileMapLayer::draw_tile`); without them a flipped edge piece drew facing
/// the wrong way. Alternative 0 is the unflipped default and is skipped.
fn collect_tile_alternative_flags(
    source_id: i32,
    source_props: &HashMap<String, Variant>,
    out: &mut HashMap<String, Variant>,
) {
    for (key, value) in source_props {
        let Some((coords, rest)) = key.split_once('/') else {
            continue;
        };
        let Some((alt, field)) = rest.split_once('/') else {
            continue;
        };
        let bit = match field {
            "flip_h" => crate::tilemap::TILE_FLIP_H,
            "flip_v" => crate::tilemap::TILE_FLIP_V,
            "transpose" => crate::tilemap::TILE_TRANSPOSE,
            _ => continue,
        };
        if !matches!(value, Variant::Bool(true)) {
            continue;
        }
        let (Some((ax, ay)), Ok(alt)) = (coords.split_once(':'), alt.trim().parse::<i32>()) else {
            continue;
        };
        let (Ok(ax), Ok(ay)) = (ax.trim().parse::<i32>(), ay.trim().parse::<i32>()) else {
            continue;
        };
        if alt == 0 {
            continue;
        }
        let k = crate::tilemap::tile_alternative_flags_key(
            source_id,
            gdcore::math::Vector2i::new(ax, ay),
            alt,
        );
        let mask = match out.get(&k) {
            Some(Variant::Int(m)) => *m,
            _ => 0,
        } | bit;
        out.insert(k, Variant::Int(mask));
    }
}

fn collect_tile_physics(
    source_id: i32,
    source_props: &HashMap<String, Variant>,
    out: &mut HashMap<String, Variant>,
) -> bool {
    let mut any = false;
    for (key, value) in source_props {
        // Split `<ax>:<ay>/<alt>/physics_layer_<n>/polygon_<m>/points`.
        let Some((coords, rest)) = key.split_once('/') else {
            continue;
        };
        let Some((ax, ay)) = coords.split_once(':') else {
            continue;
        };
        let (Ok(ax), Ok(ay)) = (ax.trim().parse::<i32>(), ay.trim().parse::<i32>()) else {
            continue;
        };
        // `rest` is `<alt>/physics_layer_<n>/polygon_<m>/<field>`.
        //
        // ⚠️ The alternative id used to be DROPPED here, so every alternative
        // of a tile wrote the SAME key and whichever `HashMap` iteration
        // visited last won. In the official 2D Platformer demo's TileSet the
        // `flip_v` alternative's box sits 10 px off the default's, so a floor
        // could collide 10 px off — differently from run to run. Alternative
        // 0 keeps the base key; every other alternative gets its own.
        let Some((alt, layer_rest)) = rest.split_once('/') else {
            continue;
        };
        let alt: i32 = alt.trim().parse().unwrap_or(0);
        if !layer_rest.starts_with("physics_layer_") {
            continue;
        }
        let atlas = gdcore::math::Vector2i::new(ax, ay);

        if layer_rest.ends_with("/polygon_0/points") {
            // Godot authors these relative to the tile's centre, which is also
            // where `tilemap_to_physics` places the body — so they carry over
            // with no rebasing.
            let points = match value {
                Variant::PackedVector2Array(p) => p.clone(),
                Variant::Array(items) => items
                    .to_vec()
                    .iter()
                    .filter_map(|v| match v {
                        Variant::Vector2(p) => Some(*p),
                        _ => None,
                    })
                    .collect(),
                _ => continue,
            };
            if points.len() >= 3 {
                let key = if alt == 0 {
                    crate::tilemap::tile_physics_points_key(source_id, atlas)
                } else {
                    crate::tilemap::tile_physics_points_alt_key(source_id, atlas, alt)
                };
                out.insert(key, Variant::PackedVector2Array(points));
                any = true;
            }
        } else if alt == 0 && layer_rest.ends_with("/polygon_0/one_way") {
            // The default tile's flag only, for the same reason as above: a
            // shared key fed by every alternative is decided by hash order.
            let one_way =
                matches!(value, Variant::Bool(true)) || matches!(value, Variant::Int(i) if *i != 0);
            out.insert(
                crate::tilemap::tile_physics_one_way_key(source_id, atlas),
                Variant::Bool(one_way),
            );
            // A tile may declare only `one_way` and rely on the default
            // full-cell shape, so this counts as collision too.
            any = true;
        }
    }
    any
}

fn encode_shape_marker(ty: &str, props: Option<&HashMap<String, Variant>>) -> String {
    let num = |k: &str| -> Option<f32> {
        match props?.get(k)? {
            Variant::Float(f) => Some(*f as f32),
            Variant::Int(i) => Some(*i as f32),
            _ => None,
        }
    };
    match ty {
        "BoxShape3D" => match props.and_then(|p| p.get("size")) {
            Some(Variant::Vector3(s)) => format!("shape:BoxShape3D|{},{},{}", s.x, s.y, s.z),
            _ => "shape:BoxShape3D".to_string(),
        },
        "SphereShape3D" => match num("radius") {
            Some(r) => format!("shape:SphereShape3D|{r}"),
            None => "shape:SphereShape3D".to_string(),
        },
        "CapsuleShape3D" | "CylinderShape3D" => {
            let r = num("radius").unwrap_or(0.5);
            let h = num("height").unwrap_or(2.0);
            format!("shape:{ty}|{r},{h}")
        }
        _ => format!("shape:{ty}"),
    }
}

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
/// Loads an `ExtResource` material's `.tres` and returns its class plus its
/// property bag, so the material pass can flatten it exactly as it already
/// flattens a `SubResource` (pat-mgixk.53).
///
/// 🔑 Before this, an `ExtResource` material did not merely lose its textures —
/// it never reached the renderer at all. `packed_scene` stores such a property
/// as the RAW REFERENCE STRING (`ExtResource("1_abc")`), so
/// `render_server_3d::resolve_material` handed that literal to
/// `material_from_path`, whose filename test for `red`/`green`/`blue` matched
/// nothing and returned the default white. The heuristic never saw a path.
///
/// ⚠️ Returns `None` on ANY failure — missing file, unreadable, unparseable —
/// rather than propagating an error. Scenes are routinely loaded in tests and
/// tools with no `res://` roots configured, so a failed read is an ordinary
/// condition; the node then keeps no `__material__*` bag and renders exactly as
/// it did before this existed.
fn ext_material_props(
    value: Option<&Variant>,
    ext_resources: &HashMap<String, ExtResourceEntry>,
) -> Option<(String, Vec<(String, Variant)>)> {
    let raw = match value? {
        Variant::String(s) => s,
        _ => return None,
    };
    let entry = ext_resources.get(&parse_ext_resource_ref(raw)?)?;

    // ⚠️ ShaderMaterial is EXCLUDED, for the same reason the SubResource branch
    // excludes it and with the same consequence if it were not: its keys are
    // `shader_parameter/*`, so a `__material__*` bag holding none of the
    // StandardMaterial3D keys would make `material3d_for_node` hand the
    // renderer a DEFAULT material where it correctly answers `None` today —
    // turning every shader-material mesh flat.
    //
    // Checked on BOTH the section's declared type and the parsed class: Godot
    // writes the BASE type on an `[ext_resource]` line (`type="Material"`), so
    // the declared type usually cannot tell the two apart and the authoritative
    // answer is in the file's own `[gd_resource type=...]`.
    if entry.res_type == "ShaderMaterial" {
        return None;
    }
    let text = gdcore::file_access::FileAccess::read_file_string(&entry.path).ok()?;
    let resource = gdresource::loader::TresLoader::new()
        .parse_str(&text, &entry.path)
        .ok()?;
    if resource.class_name == "ShaderMaterial" {
        return None;
    }
    let props = resource
        .properties()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Some((resource.class_name.clone(), props))
}

fn parse_ext_resource_ref(value: &str) -> Option<String> {
    let trimmed = value.trim();
    // Two spellings reach us: the raw `.tscn` call form stored by this parser,
    // and the `ExtResource:<id>` form `gdresource::loader::parse_variant_value`
    // produces for sub-resource property blocks (pat-0q2kx.6).
    let id = match trimmed.strip_prefix("ExtResource(") {
        Some(inner) => inner.strip_suffix(')')?.trim().trim_matches('"'),
        None => trimmed.strip_prefix("ExtResource:")?.trim(),
    };
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
/// The `[keycode, modifier_mask, …]` pairs a Shortcut sub-resource declares
/// (pat-s3rrv.175).
///
/// `events` is an Array of nested `SubResource:<id>` references to InputEventKey
/// bodies, so each entry is resolved one more level to read its `keycode` and
/// modifier flags. An event with no keycode is skipped rather than stored as 0,
/// which is a real key and would fire the shortcut on the wrong press.
///
/// Modifier mask: 1 = ctrl, 2 = shift, 4 = alt.
/// Flattens a `Skeleton2D`'s modification stack onto the node (pat-s3rrv.185).
///
/// A rig is a TWO-level sub-resource chain — the node's `modification_stack`
/// names a `SkeletonModificationStack2D`, whose `modifications/<i>` entries each
/// name a `SkeletonModification2D*` (`skeleton_modification_stack_2d.cpp:37`).
/// Neither level survived loading: `modification_stack = SubResource("…")` sat
/// on the node as an inert string and the entries were unreachable, so a rig
/// with a TwoBoneIK modifier loaded and did nothing.
///
/// 🔑 Emitted as individually-keyed properties rather than one packed array.
/// Modifications are heterogeneous — the fields TwoBoneIK needs are not the
/// fields LookAt needs — and the `shortcut` flatten beside this one records why
/// parallel index-read arrays are a trap: they are coupled through their length,
/// and a filter applied to one silently misaligns the other.
fn flatten_modification_stack(
    sub_resources: &HashMap<String, String>,
    sub_props: &HashMap<String, HashMap<String, Variant>>,
    stack_id: &str,
    out: &mut HashMap<String, Variant>,
) {
    let Some(stack) = sub_props.get(stack_id) else {
        return;
    };
    if matches!(stack.get("enabled"), Some(Variant::Bool(false))) {
        return;
    }
    let mut count = 0usize;
    // Godot writes the entries as `modifications/0`, `modifications/1`, … and
    // stops at the first gap, which is what `modification_count` bounds.
    for i in 0..64usize {
        let Some(Variant::String(reference)) = stack.get(&format!("modifications/{i}")) else {
            break;
        };
        let Some(mod_id) = reference.strip_prefix("SubResource:") else {
            break;
        };
        let Some(props) = sub_props.get(mod_id) else {
            break;
        };
        let class = sub_resources.get(mod_id).cloned().unwrap_or_default();
        out.insert(format!("__skelmod_{i}_class__"), Variant::String(class));
        // ⚠️ Only the fields a modification actually declares are copied, and
        // each keeps its own key. A missing key means "not authored", which the
        // executor distinguishes from an authored zero.
        for key in [
            "enabled",
            "target_nodepath",
            "flip_bend_direction",
            "target_minimum_distance",
            "target_maximum_distance",
            "joint_one_bone2d_node",
            "joint_two_bone2d_node",
            "joint_one_bone_idx",
            "joint_two_bone_idx",
        ] {
            if let Some(v) = props.get(key) {
                out.insert(format!("__skelmod_{i}_{key}__"), v.clone());
            }
        }
        count = i + 1;
    }
    if count > 0 {
        out.insert("__skelmod_count__".into(), Variant::Int(count as i64));
    }
}

fn shortcut_event_pairs(
    sub_props: &HashMap<String, HashMap<String, Variant>>,
    shortcut_id: &str,
) -> Vec<i32> {
    let Some(props) = sub_props.get(shortcut_id) else {
        return Vec::new();
    };
    let Some(Variant::Array(events)) = props.get("events") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ev in events.to_vec().iter() {
        let Variant::String(reference) = ev else {
            continue;
        };
        let Some(id) = reference.strip_prefix("SubResource:") else {
            continue;
        };
        let Some(key) = sub_props.get(id) else {
            continue;
        };
        let keycode = match key.get("keycode") {
            Some(Variant::Int(k)) if *k != 0 => *k as i32,
            _ => continue,
        };
        let flag = |name: &str, bit: i32| match key.get(name) {
            Some(Variant::Bool(true)) => bit,
            _ => 0,
        };
        let mods = flag("ctrl_pressed", 1) + flag("shift_pressed", 2) + flag("alt_pressed", 4);
        out.push(keycode);
        out.push(mods);
    }
    out
}

/// The actions a Shortcut's `InputEventAction` events name.
///
/// Godot's `Shortcut.events` may hold an `InputEventAction` rather than a key —
/// the official Dodge the Creeps HUD binds its Start button to
/// `InputEventAction(action = &"start_game")`, so Enter or Space start the
/// game. [`shortcut_event_pairs`] reads only `keycode`, which an action event
/// does not have, so such a shortcut flattened to nothing and never fired.
fn shortcut_actions(
    sub_props: &HashMap<String, HashMap<String, Variant>>,
    shortcut_id: &str,
) -> Vec<String> {
    let Some(Variant::Array(events)) = sub_props.get(shortcut_id).and_then(|p| p.get("events"))
    else {
        return Vec::new();
    };
    events
        .to_vec()
        .iter()
        .filter_map(|ev| {
            let Variant::String(reference) = ev else {
                return None;
            };
            let event = sub_props.get(reference.strip_prefix("SubResource:")?)?;
            match event.get("action")? {
                Variant::StringName(s) if !s.as_str().is_empty() => Some(s.as_str().to_string()),
                Variant::String(s) if !s.is_empty() => Some(s.clone()),
                _ => None,
            }
        })
        .collect()
}

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

    // Build a real AnimationPlayer for every `AnimationPlayer` node the scene
    // declared, from its `libraries`/`autoplay`, and start its autoplay clip
    // (pat-0q2kx.1). Godot does this when the player enters the tree.
    crate::animation_tscn::attach_scene_animation_players(tree, new_root_id, scene);

    // Build the graph for every `AnimationTree` node from its `tree_root`
    // sub-resources (pat-0q2kx.2). Runs after the players above because a tree
    // resolves its clip library from the player at its `anim_player` path.
    crate::animation_tree_tscn::attach_scene_animation_trees(tree, new_root_id, scene);

    // Auto-assign `current = true` to the first Camera3D in the instanced
    // scene if no camera already has it set. Godot does this automatically
    // when a Camera3D enters the viewport.
    auto_assign_current_camera(tree, new_root_id);

    // Attach a decoded stream to every VideoStreamPlayer the scene brought in
    // (pat-s3rrv.171). Done here rather than at the runner's three instancing
    // sites so every caller gets it, and idempotent so instancing a sub-scene
    // does not rewind a player that is already running.
    crate::video_stream::bind_video_players(tree);

    // Give every `resource_local_to_scene` sub-resource in the subtree we just
    // built its own copy, and run its `_setup_local_to_scene`
    // (pat-mgixk.71). Godot does this inside `SceneState::instantiate`
    // (`packed_scene.cpp` `make_local_resource`).
    //
    // ⚠️ `gdscene::local_to_scene` was a FINISHED module with no production
    // caller: its only call site in the whole workspace was its own test, so
    // every instance shared one resource and the virtual never fired in a real
    // scene load.
    //
    // Scoped to `new_root_id`, not the whole tree, and for a correctness reason
    // rather than speed: `__resref__` names its sub-resource by a SCENE-LOCAL
    // id, so a whole-tree pass would resolve another instance's refs against
    // THIS scene's `sub_resource_props` and could match an unrelated resource.
    // Here rather than at the callers, for the same reason
    // `bind_video_players` above is.
    crate::local_to_scene::setup_local_to_scene_in(tree, scene, new_root_id);
    Ok(new_root_id)
}

/// Adds all nodes from a [`PackedScene::instance_with_subscenes()`] call
/// into a [`crate::SceneTree`] under the given parent, with recursive
/// sub-scene instancing.
///
/// The `resolve_scene` callback is invoked for each `instance=ExtResource`
/// reference, receiving the `res://` path and returning the parsed sub-scene.
// ---------------------------------------------------------------------------
// InstancePlaceholder runtime (pat-0q2kx.139)
// ---------------------------------------------------------------------------

/// The `res://` path an `InstancePlaceholder` stands in for
/// (`InstancePlaceholder.get_instance_path`).
pub fn placeholder_instance_path(
    tree: &crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
) -> Option<String> {
    let node = tree.get_node(node_id)?;
    if node.class_name() != "InstancePlaceholder" {
        return None;
    }
    match node.get_property("_instance_placeholder_path") {
        Variant::String(p) => Some(p),
        _ => None,
    }
}

/// The property overrides the placeholder is holding for the scene it stands in
/// for (`InstancePlaceholder.get_stored_values`).
///
/// Stored as a flat `[key, value, key, value, …]` array because `Variant` has no
/// ordered-map kind that survives the scene round-trip; this rebuilds the pairs.
pub fn placeholder_stored_values(
    tree: &crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
) -> Vec<(String, Variant)> {
    let Some(node) = tree.get_node(node_id) else {
        return Vec::new();
    };
    let Variant::Array(flat) = node.get_property("_stored_values") else {
        return Vec::new();
    };
    flat.to_vec()
        .chunks_exact(2)
        .filter_map(|pair| match &pair[0] {
            Variant::String(k) => Some((k.clone(), pair[1].clone())),
            _ => None,
        })
        .collect()
}

/// Replaces an `InstancePlaceholder` with the real sub-scene
/// (`InstancePlaceholder.create_instance`).
///
/// The placeholder's stored values are applied to the instanced root, matching
/// Godot: the whole point of a placeholder is to defer the load while keeping
/// the edits the scene author made to it. The placeholder node is removed and
/// the new subtree takes its place under the same parent.
///
/// Returns the new root's id, or `None` when the node is not a placeholder or
/// its scene cannot be resolved.
pub fn placeholder_create_instance<F>(
    tree: &mut crate::scene_tree::SceneTree,
    node_id: crate::node::NodeId,
    resolve_scene: &F,
) -> Option<crate::node::NodeId>
where
    F: Fn(&str) -> Option<PackedScene>,
{
    let path = placeholder_instance_path(tree, node_id)?;
    let stored = placeholder_stored_values(tree, node_id);
    let scene = resolve_scene(&path)?;
    let parent = tree.get_node(node_id).and_then(|n| n.parent())?;

    let new_root =
        add_packed_scene_to_tree_with_subscenes(tree, parent, &scene, resolve_scene).ok()?;
    if let Some(node) = tree.get_node_mut(new_root) {
        for (key, value) in stored {
            node.set_property(&key, value);
        }
    }
    // The placeholder has been superseded; leaving it would double the node.
    let _ = tree.remove_node(node_id);
    Some(new_root)
}

/// Instantiates `scene` **outside** the tree and returns its detached root,
/// mirroring Godot's `PackedScene.instantiate()`
/// (`scene/resources/packed_scene.cpp SceneState::instantiate`).
///
/// Patina used to parent the instance under `/root` on creation, so every node
/// got ENTER_TREE/READY on insertion — before its children existed — and the
/// caller's `add_child` then reparented it, replaying the whole lifecycle a
/// second time (pat-mgixk.61).
///
/// The returned root has no parent and is not `inside_tree`, so no node in it
/// has run `_enter_tree` or `_ready`. Those fire once, child-first, when a
/// caller hands the root to `add_child` — which is exactly Godot's contract and
/// what lets a script set properties on the instance *before* its
/// `_enter_tree` observes them.
///
/// # Why a holder node rather than a bespoke builder
///
/// [`SceneTree::add_child`] only runs the lifecycle when the parent
/// `is_inside_tree()`. Building the whole scene under a freshly
/// [`create_node`](crate::scene_tree::SceneTree::create_node)d node — which is
/// in the arena but has **no parent**, so it is not inside the tree — therefore
/// fires nothing for any descendant, at any depth. The instance is then cut
/// loose and the holder dropped.
///
/// ⚠️ Reusing [`add_packed_scene_to_tree_with_subscenes`] matters: it also wires
/// the scene's signal connections and attaches its `AnimationPlayer` resources.
/// A hand-rolled detached builder would silently skip both, and nothing in a
/// spawn test would notice until a connected signal failed to fire in a game.
pub fn instantiate_packed_scene_detached<F>(
    tree: &mut crate::scene_tree::SceneTree,
    scene: &PackedScene,
    resolve_scene: &F,
) -> EngineResult<crate::node::NodeId>
where
    F: Fn(&str) -> Option<PackedScene>,
{
    // Not inside the tree: `create_node` inserts into the arena without a
    // parent, which is the whole point.
    let holder = tree.create_node("Node", "@PackedSceneInstanceHolder");
    let built = add_packed_scene_to_tree_with_subscenes(tree, holder, scene, resolve_scene);
    let root = match built {
        Ok(root) => root,
        Err(e) => {
            // Never leave the holder behind on failure; it would be an
            // unreachable node that `node_count` still counts.
            tree.take_node(holder);
            return Err(e);
        }
    };

    // Cut the instance loose, then drop the holder. Order matters: the root must
    // stop pointing at the holder BEFORE the holder leaves the arena, or the
    // root keeps a parent id that resolves to nothing — and
    // `SceneAccess::add_child` reads `parent().is_some()` to decide between
    // reparenting and inserting, so a dangling parent would send a detached
    // instance down the reparent path and replay the lifecycle after all.
    if let Some(holder_node) = tree.get_node_mut(holder) {
        holder_node.remove_child_id(root);
    }
    if let Some(root_node) = tree.get_node_mut(root) {
        root_node.set_parent(None);
    }
    tree.take_node(holder);

    Ok(root)
}

thread_local! {
    /// Scenes [`load_scene_from_res`] has read during the current instancing
    /// session (`None` = unreadable or cyclic), so a level that instances one
    /// bullet scene a thousand times parses it once.
    static SUBSCENE_CACHE: std::cell::RefCell<HashMap<String, Option<PackedScene>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// Starts a fresh [`load_scene_from_res`] session: scenes are re-read, so an
/// edit on disk between two scene changes is seen.
pub fn begin_subscene_session() {
    SUBSCENE_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// The `res://` paths of every scene `scene` instances directly.
fn instanced_scene_paths(scene: &PackedScene) -> Vec<String> {
    scene
        .templates
        .iter()
        .filter_map(|t| t.instance.as_deref())
        .filter_map(|r| scene.resolve_ext_resource_path(r))
        .map(str::to_string)
        .collect()
}

/// An external TileSet `.tres`, parsed into the tables
/// [`flatten_tile_set_into`] walks.
struct TileSetTres {
    /// The `[resource]` section's own properties (the TileSet).
    set_props: HashMap<String, Variant>,
    sub_resource_props: HashMap<String, HashMap<String, Variant>>,
    ext_resources: HashMap<String, ExtResourceEntry>,
}

/// Reads a TileSet `.tres` through the scene parser: its `[resource]` section
/// is relabelled as one more sub-resource so every value — `SubResource(..)` /
/// `ExtResource(..)` references, packed arrays, the `0:0/0/...` tile keys —
/// parses exactly as it would inline in a `.tscn`.
fn read_tile_set_tres(path: &str) -> Option<TileSetTres> {
    const ROOT: &str = "__tres_resource__";
    let text = match gdcore::file_access::read_from_packs(path) {
        Some(bytes) => String::from_utf8(bytes).ok()?,
        None => std::fs::read_to_string(gdcore::resolve_path(path).ok()?).ok()?,
    };
    let relabelled: String = text
        .lines()
        .map(|line| {
            if line.trim() == "[resource]" {
                format!("[sub_resource type=\"TileSet\" id=\"{ROOT}\"]")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // `from_tscn` requires a node; a placeholder root satisfies it and is
    // discarded — only the resource tables are kept.
    let with_root = format!("{relabelled}\n\n[node name=\"{ROOT}\" type=\"Node\"]\n");
    let mut scene = PackedScene::from_tscn(&with_root).ok()?;
    let set_props = scene.sub_resource_props.remove(ROOT)?;
    Some(TileSetTres {
        set_props,
        sub_resource_props: scene.sub_resource_props,
        ext_resources: scene.ext_resources,
    })
}

/// Flattens a TileSet (its tile size, custom data layers and every atlas
/// source's texture, region, per-tile physics, navigation and occlusion) onto
/// a TileMapLayer node's properties under `__tile_set__*` (pat-0q2kx.108), so
/// the renderer and physics server can resolve cells from the tree alone.
/// `sub_resource_props` / `ext_resources` are the tables of the file that
/// DECLARES the tile set — the scene for an inline one, the `.tres` for an
/// external one.
fn flatten_tile_set_into(
    props: &mut HashMap<String, Variant>,
    set_props: &HashMap<String, Variant>,
    sub_resource_props: &HashMap<String, HashMap<String, Variant>>,
    ext_resources: &HashMap<String, ExtResourceEntry>,
) {
    // `TileSet.tile_size` is the map's cell size. Godot omits it when it
    // is the 16x16 default, so an absent property is not "no tile set".
    let tile_size = match set_props.get("tile_size") {
        Some(Variant::Vector2i(v)) => *v,
        Some(Variant::Vector2(v)) => gdcore::math::Vector2i::new(v.x as i32, v.y as i32),
        _ => gdcore::math::Vector2i::new(16, 16),
    };
    props.insert(
        format!("{TILE_SET_PREFIX}tile_size"),
        Variant::Vector2i(tile_size),
    );

    // Godot names custom data layers on the TILESET
    // (`custom_data_layer_<i>/name`, tile_set.cpp:4211) and refers to
    // them per tile by INDEX (`custom_data_<i>`, :6848). Patina's
    // companions are keyed by NAME, so the index -> name map has to be
    // built here, before any source is walked.
    //
    // ⚠️ Densely numbered from 0 like every Godot array property, so
    // stop at the first gap rather than scanning an arbitrary range.
    let mut custom_data_layers: Vec<String> = Vec::new();
    // Godot also declares each layer's Variant TYPE
    // (`custom_data_layer_<i>/type`). Carried through verbatim so a
    // round trip does not silently strip it: values are still copied
    // as authored, and nothing is ever inferred — a layer the file
    // typed keeps its type, a layer it did not stays untyped (-1).
    let mut custom_data_types: Vec<i32> = Vec::new();
    for index in 0.. {
        match set_props.get(&format!("custom_data_layer_{index}/name")) {
            Some(Variant::String(name)) if !name.is_empty() => {
                custom_data_layers.push(name.clone());
                custom_data_types.push(
                    match set_props.get(&format!("custom_data_layer_{index}/type")) {
                        Some(Variant::Int(t)) => *t as i32,
                        _ => -1,
                    },
                );
            }
            _ => break,
        }
    }
    if !custom_data_layers.is_empty() {
        props.insert(
            crate::tilemap::TILE_CUSTOM_DATA_LAYERS.to_string(),
            Variant::PackedStringArray(custom_data_layers.clone()),
        );
        // Only when the file actually typed something. An all `-1`
        // array would be indistinguishable from an editor-authored
        // TileSet and would make the saver emit nothing anyway, so
        // omitting it keeps the companion set minimal.
        if custom_data_types.iter().any(|t| *t >= 0) {
            props.insert(
                crate::tilemap::TILE_CUSTOM_DATA_TYPES.to_string(),
                Variant::PackedInt32Array(custom_data_types),
            );
        }
    }

    // The physics layer's collision bits. Godot defaults both to 1; a TileSet
    // that moves its tiles to another layer (the official 2D Platformer
    // demo's tiles are on layer 16, its player masks 30) collided with
    // NOTHING while every tile body was created on layer 1.
    for (key, companion) in [
        ("physics_layer_0/collision_layer", crate::tilemap::TILE_PHYSICS_COLLISION_LAYER),
        ("physics_layer_0/collision_mask", crate::tilemap::TILE_PHYSICS_COLLISION_MASK),
    ] {
        if let Some(Variant::Int(bits)) = set_props.get(key) {
            props.insert(companion.to_string(), Variant::Int(*bits));
        }
    }

    let mut source_ids: Vec<i32> = Vec::new();
    let mut physics_source_ids: Vec<i32> = Vec::new();
    let mut nav_source_ids: Vec<i32> = Vec::new();
    let mut occl_source_ids: Vec<i32> = Vec::new();
    for (key, value) in set_props {
        // `sources/3 = SubResource("TileSetAtlasSource_x")`
        let Some(id_str) = key.strip_prefix("sources/") else {
            continue;
        };
        let Ok(source_id) = id_str.trim().parse::<i32>() else {
            continue;
        };
        let Variant::String(source_ref) = value else {
            continue;
        };
        let Some(source_sub) = source_ref.strip_prefix("SubResource:") else {
            continue;
        };
        let Some(source_props) = sub_resource_props.get(source_sub) else {
            continue;
        };
        source_ids.push(source_id);

        let prefix = format!("{TILE_SET_PREFIX}source_{source_id}_");
        // `texture = ExtResource("id")` — resolve to the real path, the
        // same value a Sprite2D's `texture` ends up holding.
        if let Some(Variant::String(tex)) = source_props.get("texture") {
            let path = tex
                .strip_prefix("ExtResource:")
                .and_then(|id| ext_resources.get(id).map(|e| e.path.clone()))
                .unwrap_or_else(|| tex.clone());
            props
                .insert(format!("{prefix}texture"), Variant::String(path));
        }
        for key in ["texture_region_size", "margins", "separation"] {
            if let Some(value) = source_props.get(key) {
                props
                    .insert(format!("{prefix}{key}"), value.clone());
            }
        }

        // Per-tile collision (pat-0q2kx.117). A real TileSetAtlasSource
        // writes one key per tile per physics layer:
        //   `<ax>:<ay>/<alt>/physics_layer_0/polygon_0/points`
        //   `<ax>:<ay>/<alt>/physics_layer_0/polygon_0/one_way`
        // Flattened onto the node the physics server can build the
        // tile's static bodies from the tree alone, exactly as
        // `__shape__*` lets it build a CollisionShape2D's body.
        collect_tile_custom_data(
            source_id,
            source_props,
            &custom_data_layers,
            props,
        );
        collect_tile_alternative_flags(source_id, source_props, props);
        if collect_tile_physics(source_id, source_props, props) {
            physics_source_ids.push(source_id);
        }
        if collect_tile_navigation(
            source_id,
            source_props,
            sub_resource_props,
            tile_size,
            props,
        ) {
            nav_source_ids.push(source_id);
        }
        if collect_tile_occlusion(
            source_id,
            source_props,
            sub_resource_props,
            tile_size,
            props,
        ) {
            occl_source_ids.push(source_id);
        }
    }
    source_ids.sort_unstable();
    props.insert(
        format!("{TILE_SET_PREFIX}source_ids"),
        Variant::PackedInt32Array(source_ids),
    );
    physics_source_ids.sort_unstable();
    physics_source_ids.dedup();
    props.insert(
        crate::tilemap::TILE_PHYSICS_SOURCES.to_string(),
        Variant::PackedInt32Array(physics_source_ids),
    );
    nav_source_ids.sort_unstable();
    nav_source_ids.dedup();
    props.insert(
        crate::tilemap::TILE_NAV_SOURCES.to_string(),
        Variant::PackedInt32Array(nav_source_ids),
    );
    occl_source_ids.sort_unstable();
    occl_source_ids.dedup();
    props.insert(
        crate::tilemap::TILE_OCCLUDER_SOURCES.to_string(),
        Variant::PackedInt32Array(occl_source_ids),
    );
}

fn read_scene_file(path: &str) -> Option<PackedScene> {
    // Text `.tscn` or binary `.scn` (pat-33wmr): a `read_to_string` here made
    // every `instance=` of a binary scene come up as an empty `Node`.
    let text = crate::binary_scene::read_scene_source(path)?;
    PackedScene::from_tscn(&text).ok()
}

/// Whether `scene` (loaded from `path`) reaches `path` again through its chain
/// of instances. Godot refuses such a scene as a cyclic instance; instancing it
/// would recurse until the stack overflowed.
fn instances_itself(path: &str, scene: &PackedScene) -> bool {
    let mut pending = instanced_scene_paths(scene);
    let mut seen = std::collections::HashSet::new();
    while let Some(next) = pending.pop() {
        if next == path {
            return true;
        }
        if !seen.insert(next.clone()) {
            continue;
        }
        if let Some(sub) = read_scene_file(&next) {
            pending.extend(instanced_scene_paths(&sub));
        }
    }
    false
}

/// Loads a `.tscn` by Godot path, for expanding `instance=ExtResource(..)`
/// nodes at runtime: mounted packs first (an exported game), then the file
/// under the configured `res://` root. `None` when it cannot be read or parsed,
/// or when it instances itself (directly or through other scenes) — a
/// non-cyclic instance graph is at most as deep as it has distinct files, so
/// the recursion this feeds always terminates.
pub fn load_scene_from_res(path: &str) -> Option<PackedScene> {
    if let Some(cached) = SUBSCENE_CACHE.with(|cache| cache.borrow().get(path).cloned()) {
        return cached;
    }
    let loaded = read_scene_file(path).filter(|scene| {
        let cyclic = instances_itself(path, scene);
        if cyclic {
            tracing::warn!("{path} instances itself; refusing the cyclic instance");
        }
        !cyclic
    });
    SUBSCENE_CACHE.with(|cache| cache.borrow_mut().insert(path.to_string(), loaded.clone()));
    loaded
}

/// One scene reached while flattening `instance=`/inherited sub-scenes: the
/// `PackedScene` itself, where its root landed in the flat node list, and the
/// nodes its own templates created. See [`PackedScene::instance_levels`].
struct SceneLevel {
    scene: PackedScene,
    root: usize,
    own: Vec<usize>,
}

impl SceneLevel {
    fn offset_by(mut self, by: usize) -> Self {
        self.root += by;
        for idx in &mut self.own {
            *idx += by;
        }
        self
    }
}

/// Where [`mount_packed_scene`] puts a scene's root.
#[derive(Clone, Copy)]
enum MountPoint {
    /// A new node, added as the last child of this parent.
    Under(crate::node::NodeId),
    /// Into this EXISTING node — an `instance=` placeholder that a plain
    /// [`add_packed_scene_to_tree`] left childless. It keeps its id, so every
    /// signal already wired to it survives, and its own (authored) property
    /// values win over the sub-scene's.
    Into(crate::node::NodeId),
    /// A new node with NO parent: the whole scene is built in the arena but
    /// outside the tree, so nothing enters or readies until the caller
    /// `add_child`s the finished root — Godot's `instantiate()` followed by
    /// one `add_child`.
    Detached,
}

/// Instances `scene` with every `instance=`/inherited sub-scene expanded, and
/// gives EACH scene level what a scene brings besides nodes: its connections
/// (resolved relative to that level's own root), its AnimationPlayers and
/// AnimationTrees, and its local-to-scene resources — each pass over just the
/// nodes that level created, because `SubResource` ids are scene-local.
fn mount_packed_scene<F>(
    tree: &mut crate::scene_tree::SceneTree,
    at: MountPoint,
    scene: &PackedScene,
    resolve_scene: &F,
) -> EngineResult<crate::node::NodeId>
where
    F: Fn(&str) -> Option<PackedScene>,
{
    use crate::node::NodeId;

    let mut levels = Vec::new();
    let (nodes, own) = scene.instance_levels(resolve_scene, &mut levels)?;
    if nodes.is_empty() {
        return Err(EngineError::InvalidOperation(
            "instanced scene produced no nodes".into(),
        ));
    }

    // Copies a detached template node into a fresh tree node. Fresh rather
    // than moved: the flat node's child-id list points into the flat list.
    fn fresh_copy(node: &Node) -> Node {
        let mut copy = Node::new(node.name(), node.class_name());
        for (key, value) in node.properties() {
            copy.set_property(key, value.clone());
        }
        for group in node.groups() {
            copy.add_to_group(group.clone());
        }
        copy.set_unique_name(node.is_unique_name());
        copy
    }

    // Folds a sub-scene node into the node that already stands at its path —
    // the placeholder, or the outer scene's override entry for it. The class
    // comes from the sub-scene (the existing node is typeless); every value the
    // existing node already holds was authored by the outer scene (or edited
    // in the editor), so it wins.
    fn merge_into(tree: &mut crate::scene_tree::SceneTree, target: NodeId, src: &Node) {
        let Some(existing) = tree.get_node_mut(target) else {
            return;
        };
        existing.set_class_name(src.class_name());
        for (key, value) in src.properties() {
            if !existing.has_property(key) {
                existing.set_property(key, value.clone());
            }
        }
        for group in src.groups() {
            existing.add_to_group(group.clone());
        }
        if src.is_unique_name() {
            existing.set_unique_name(true);
        }
    }

    let mut final_ids: Vec<NodeId> = Vec::with_capacity(nodes.len());
    let mut old_to_new: HashMap<NodeId, NodeId> = HashMap::new();
    let root_id = match at {
        MountPoint::Under(parent) => tree.add_child(parent, fresh_copy(&nodes[0]))?,
        MountPoint::Detached => tree.insert_detached(fresh_copy(&nodes[0])),
        MountPoint::Into(target) => {
            merge_into(tree, target, &nodes[0]);
            target
        }
    };
    old_to_new.insert(nodes[0].id(), root_id);
    final_ids.push(root_id);

    // Children each merged parent already had before the sub-scene arrived:
    // the outer scene's override entries and its own additions.
    let mut prior_children: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    let is_into = matches!(at, MountPoint::Into(_));

    for node in &nodes[1..] {
        let old_parent = node.parent().ok_or_else(|| {
            EngineError::InvalidOperation(format!("instanced node '{}' has no parent", node.name()))
        })?;
        let &parent = old_to_new.get(&old_parent).ok_or_else(|| {
            EngineError::InvalidOperation(format!(
                "parent for '{}' not yet added to tree",
                node.name()
            ))
        })?;

        let prior = if is_into {
            prior_children
                .entry(parent)
                .or_insert_with(|| {
                    tree.get_node(parent)
                        .map(|p| p.children().to_vec())
                        .unwrap_or_default()
                })
                .clone()
        } else {
            Vec::new()
        };
        let existing = prior.into_iter().find(|&child| {
            tree.get_node(child).is_some_and(|c| c.name() == node.name())
        });

        let id = match existing {
            Some(existing) => {
                merge_into(tree, existing, node);
                existing
            }
            None => {
                let mut copy = fresh_copy(node);
                copy.set_owner(Some(root_id));
                tree.add_child(parent, copy)?
            }
        };
        old_to_new.insert(node.id(), id);
        final_ids.push(id);
    }

    // Godot orders an instance's own children ahead of anything the outer
    // scene added to it. `add_child` appended the sub-scene's after the outer
    // ones, so move each merged parent's outer-only children to the end.
    for (parent, prior) in &prior_children {
        let sub_children: std::collections::HashSet<NodeId> = final_ids.iter().copied().collect();
        let outer_only: Vec<NodeId> = prior
            .iter()
            .copied()
            .filter(|child| {
                // A prior child the sub-scene merged into belongs to the
                // sub-scene's order; only genuine outer additions move.
                !sub_children.contains(child)
            })
            .collect();
        for child in outer_only {
            let last = tree
                .get_node(*parent)
                .map(|p| p.children().len().saturating_sub(1))
                .unwrap_or(0);
            let _ = tree.move_child(*parent, child, last);
        }
    }

    // Godot's ownership: a node is owned by the root of the scene that
    // declared it, so an instance's inner nodes belong to the INSTANCE root,
    // not the outermost scene (`%Unique` lookups are scoped by owner). Levels
    // are inner-first, so an outer level never overwrites an inner choice —
    // each level only touches the nodes it created itself.
    // The mounted scene itself last: under `Into`, a node merged into an
    // outer override entry still carries the OUTER owner until this runs.
    let top = (0usize, &own);
    for (root_idx, level_own) in levels.iter().map(|l| (l.root, &l.own)).chain([top]) {
        let level_root = final_ids[root_idx];
        for &idx in level_own {
            let id = final_ids[idx];
            if id != level_root {
                if let Some(node) = tree.get_node_mut(id) {
                    node.set_owner(Some(level_root));
                }
            }
        }
    }

    // An INSTANCE root is owned by the scene that instanced it — the nearest
    // enclosing level root (or the mounted scene's root). The loop above skips
    // each level's own root (`id != level_root`), so a nested instance root kept
    // the outermost root it was created with: `Sword` (weapon.tscn, instanced by
    // player.tscn, instanced by world.tscn) was owned by World instead of Hero,
    // and `%Unique` lookups scoped by owner leaked across scene boundaries.
    let level_roots: std::collections::HashSet<NodeId> = levels
        .iter()
        .map(|l| final_ids[l.root])
        .chain([root_id])
        .collect();
    for level in &levels {
        let inner = final_ids[level.root];
        let mut cursor = tree.get_node(inner).and_then(|n| n.parent());
        while let Some(id) = cursor {
            if level_roots.contains(&id) {
                if let Some(node) = tree.get_node_mut(inner) {
                    node.set_owner(Some(id));
                }
                break;
            }
            cursor = tree.get_node(id).and_then(|n| n.parent());
        }
    }

    // Inner levels first, as Godot wires a sub-scene while instancing it,
    // before the scene that instanced it.
    let ids_of = |indices: &[usize]| -> Vec<NodeId> {
        indices.iter().filter_map(|&i| final_ids.get(i).copied()).collect()
    };
    for level in &levels {
        finish_scene_level(tree, &level.scene, final_ids[level.root], &ids_of(&level.own));
    }
    finish_scene_level(tree, scene, root_id, &ids_of(&own));

    // A camera becomes current, and a video player binds, as it ENTERS the
    // tree; a detached build has not entered yet, so its caller runs these
    // after attaching (see `finish_attached_scene`).
    if !matches!(at, MountPoint::Detached) {
        finish_attached_scene(tree, root_id);
    }
    Ok(root_id)
}

/// The post-entry half of a scene mount: current-camera assignment and video
/// player binding. Run once the scene root is inside the tree.
pub fn finish_attached_scene(tree: &mut crate::scene_tree::SceneTree, root_id: crate::node::NodeId) {
    auto_assign_current_camera(tree, root_id);
    crate::video_stream::bind_video_players(tree);
}

/// Builds `scene` (sub-scenes expanded, as
/// [`add_packed_scene_to_tree_with_subscenes`]) as a DETACHED subtree: in the
/// arena, parentless, never entered. Godot's `PackedScene::instantiate()`.
///
/// 🔑 Assembling a scene under a live parent node-by-node made each `add_child`
/// enter and ready its node before that node had children, so READY ran
/// parent-first and every node was entered once per assembly pass. Build here,
/// prepare (attach scripts), then hand the root to ONE `add_child`: ENTER
/// top-down then READY bottom-up, exactly once, as `Node::_propagate_*` does.
/// Finish with [`finish_attached_scene`] after attaching.
pub fn instance_packed_scene_detached<F>(
    tree: &mut crate::scene_tree::SceneTree,
    scene: &PackedScene,
    resolve_scene: &F,
) -> EngineResult<crate::node::NodeId>
where
    F: Fn(&str) -> Option<PackedScene>,
{
    mount_packed_scene(tree, MountPoint::Detached, scene, resolve_scene)
}

/// Godot's ownership over a FLAT instanced list: each node is owned by the root
/// of the scene that declared it, and an instance root by the nearest
/// enclosing scene root. The flat list came back with every node owned by the
/// outermost root, so a sub-scene's `Sprite` claimed `World` as its owner and
/// `%Unique` names leaked across scene boundaries. Mirrors the tree-side pass in
/// `mount_packed_scene`.
fn apply_level_owners(nodes: &mut [Node], levels: &[SceneLevel], own: &[usize]) {
    use crate::node::NodeId;
    if nodes.is_empty() {
        return;
    }
    let ids: Vec<NodeId> = nodes.iter().map(|n| n.id()).collect();
    let index_of: HashMap<NodeId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let top_root = 0usize;
    for (root_idx, level_own) in levels.iter().map(|l| (l.root, &l.own)).chain([(top_root, &own.to_vec())]) {
        let owner = ids[root_idx];
        for &idx in level_own {
            if idx != root_idx && idx < nodes.len() {
                nodes[idx].set_owner(Some(owner));
            }
        }
    }
    let roots: std::collections::HashSet<usize> =
        levels.iter().map(|l| l.root).chain([top_root]).collect();
    for level in levels {
        let mut cursor = nodes[level.root].parent().and_then(|p| index_of.get(&p).copied());
        while let Some(i) = cursor {
            if roots.contains(&i) {
                let owner = ids[i];
                nodes[level.root].set_owner(Some(owner));
                break;
            }
            cursor = nodes[i].parent().and_then(|p| index_of.get(&p).copied());
        }
    }
}

/// What one scene level brings besides its nodes, applied once its nodes (and
/// every sub-scene's) are in the tree.
fn finish_scene_level(
    tree: &mut crate::scene_tree::SceneTree,
    scene: &PackedScene,
    level_root: crate::node::NodeId,
    own: &[crate::node::NodeId],
) {
    wire_connections(tree, level_root, &scene.connections);
    crate::animation_tscn::attach_scene_animation_players_for(tree, own, scene);
    crate::animation_tree_tscn::attach_scene_animation_trees_for(tree, own, scene);
    crate::local_to_scene::setup_local_to_scene_for(tree, scene, own);
}

/// Adds `scene` under `parent_id` with every `instance=ExtResource(..)` and
/// inherited base expanded through `resolve_scene`.
///
/// This is how Godot's `SceneState::instantiate` builds a scene, and it is what
/// every RUNTIME load must use. [`add_packed_scene_to_tree`] leaves each
/// instanced sub-scene as a childless placeholder `Node` — right for the
/// editor's edited scene, which the saver must write back as `instance=`, and
/// wrong for a running game: a main scene that instances its Player and HUD
/// booted without either.
///
/// Each sub-scene level is wired with its OWN connections (relative to its own
/// root) and gets its own AnimationPlayer/AnimationTree/local-to-scene passes.
/// The previous implementation wired only the top scene's connections, so a
/// sub-scene's internal signals — a HUD button's `pressed`, a Player's
/// `body_entered` — never fired.
pub fn add_packed_scene_to_tree_with_subscenes<F>(
    tree: &mut crate::scene_tree::SceneTree,
    parent_id: crate::node::NodeId,
    scene: &PackedScene,
    resolve_scene: &F,
) -> EngineResult<crate::node::NodeId>
where
    F: Fn(&str) -> Option<PackedScene>,
{
    mount_packed_scene(tree, MountPoint::Under(parent_id), scene, resolve_scene)
}

/// [`add_packed_scene_to_tree_with_subscenes`] resolving sub-scenes from
/// `res://` (packs first, then the project root) in a fresh subscene session.
/// The one call a runtime boot path needs.
pub fn add_packed_scene_to_tree_from_res(
    tree: &mut crate::scene_tree::SceneTree,
    parent_id: crate::node::NodeId,
    scene: &PackedScene,
) -> EngineResult<crate::node::NodeId> {
    begin_subscene_session();
    add_packed_scene_to_tree_with_subscenes(tree, parent_id, scene, &load_scene_from_res)
}

/// Property marking an `instance=` placeholder that
/// [`expand_instance_placeholders`] has already expanded, so a second pass is
/// a no-op. Runtime-only; the saver never writes it.
pub const INSTANCE_EXPANDED_KEY: &str = "_instance_expanded";

/// Node property listing the `.tscn` keys authored ABOVE the node's
/// `script = …` line that its class does not have — values Godot drops because
/// it sets them before the script exists. Consumed by `SceneTree::attach_script`.
pub const PROPS_BEFORE_SCRIPT_KEY: &str = "_props_before_script";

/// Expands, in place, every `instance=` placeholder in the subtree at `root`
/// that a plain [`add_packed_scene_to_tree`] left childless — the shape of the
/// editor's edited scene. Used to turn a CLONE of that scene into the tree a
/// game runs (editor Play), which must hold the instanced Player, HUD, enemies.
///
/// Each placeholder keeps its `NodeId` (connections already wired to it
/// survive) and its values (authored overrides and unsaved editor edits win
/// over the sub-scene's); its existing children — the outer scene's override
/// entries — merge into the sub-scene's nodes of the same name.
///
/// `outer_connections` are the outer scene's connections: any whose endpoint
/// sits INSIDE an instance could not resolve before expansion and is wired
/// now. Returns how many placeholders were expanded.
pub fn expand_instance_placeholders<F>(
    tree: &mut crate::scene_tree::SceneTree,
    root: crate::node::NodeId,
    outer_connections: &[SceneConnection],
    resolve_scene: &F,
) -> usize
where
    F: Fn(&str) -> Option<PackedScene>,
{
    let resolves = |tree: &crate::scene_tree::SceneTree, path: &str| {
        path == "." || tree.get_node_relative(root, path).is_some()
    };
    let unresolved: Vec<SceneConnection> = outer_connections
        .iter()
        .filter(|c| !resolves(tree, &c.from_path) || !resolves(tree, &c.to_path))
        .cloned()
        .collect();

    let mut ids = Vec::new();
    tree.collect_subtree_top_down(root, &mut ids);
    let mut expanded = 0;
    for id in ids {
        let path = match tree.get_node(id) {
            Some(node) if !node.has_property(INSTANCE_EXPANDED_KEY) => {
                match node.get_property("_instance_path") {
                    Variant::String(path) => path,
                    _ => continue,
                }
            }
            _ => continue,
        };
        let Some(sub) = resolve_scene(&path) else {
            tracing::warn!("instanced scene '{path}' could not be loaded; left as a placeholder");
            continue;
        };
        match mount_packed_scene(tree, MountPoint::Into(id), &sub, resolve_scene) {
            Ok(_) => {
                if let Some(node) = tree.get_node_mut(id) {
                    node.set_property(INSTANCE_EXPANDED_KEY, Variant::Bool(true));
                }
                expanded += 1;
            }
            Err(e) => tracing::warn!("instanced scene '{path}' failed to expand: {e}"),
        }
    }

    let now_resolvable: Vec<SceneConnection> = unresolved
        .into_iter()
        .filter(|c| resolves(tree, &c.from_path) && resolves(tree, &c.to_path))
        .collect();
    wire_connections(tree, root, &now_resolvable);
    expanded
}


/// Automatically assigns `current = true` to the first Camera3D in the
/// subtree rooted at `root_id`, if no Camera3D already has `current` set.
///
/// This matches Godot's behavior where the first Camera3D entering the
/// viewport becomes the active camera.
fn auto_assign_current_camera(tree: &mut crate::scene_tree::SceneTree, root_id: NodeId) {
    let mut subtree = Vec::new();
    tree.collect_subtree_top_down(root_id, &mut subtree);

    // Check if any camera already has current=true.
    let any_current = subtree.iter().any(|&nid| {
        tree.get_node(nid)
            .map(|n| {
                n.class_name() == "Camera3D"
                    && matches!(n.get_property("current"), Variant::Bool(true))
            })
            .unwrap_or(false)
    });

    if any_current {
        return;
    }

    // Find the first Camera3D and make it current.
    for &nid in &subtree {
        if let Some(node) = tree.get_node_mut(nid) {
            if node.class_name() == "Camera3D" {
                node.set_property("current", Variant::Bool(true));
                return;
            }
        }
    }
}

/// Godot `Object.ConnectFlags::CONNECT_DEFERRED` — the callback runs at the
/// next idle flush rather than inside `emit_signal`.
pub const CONNECT_DEFERRED: u32 = 1;
/// Godot `Object.ConnectFlags::CONNECT_ONE_SHOT` — the connection disconnects
/// itself after firing once.
pub const CONNECT_ONE_SHOT: u32 = 4;

/// Wires signal connections from a packed scene into the scene tree.
///
/// After a packed scene has been instanced and added to the tree via
/// [`add_packed_scene_to_tree`], call this function to resolve the
/// `from`/`to` paths in each [`SceneConnection`] to [`NodeId`]s and
/// register the connections in the tree's signal stores.
///
/// - `"."` in `from_path` or `to_path` refers to the scene root.
/// - Non-existent paths produce a warning but do not cause an error.
/// - The connection's authored `flags`, `binds` and `unbinds` are applied, so a
///   deferred connection defers and a one-shot connection fires once
///   (pat-0q2kx.65). `CONNECT_PERSIST` (2) and `CONNECT_REFERENCE_COUNTED` (8)
///   are parsed but do not affect dispatch, matching the rest of the engine.
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
                // The authored flags/binds/unbinds were parsed but thrown away
                // here, so a `flags=1` connection fired immediately instead of
                // at the deferred flush, a `flags=4` one fired on every emit,
                // and bound arguments never reached the callback (pat-0q2kx.65).
                let mut connection = Connection::new(to_object_id, &conn.method_name)
                    .with_binds(conn.binds.clone())
                    .with_unbinds(conn.unbinds);
                if conn.flags & CONNECT_DEFERRED != 0 {
                    connection = connection.as_deferred();
                }
                if conn.flags & CONNECT_ONE_SHOT != 0 {
                    connection = connection.as_one_shot();
                }
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

// ───────────────────────────────────────────────────────────────────────────
// pat-nzgdu.167: SceneState — read-only introspection over a parsed PackedScene
// (Godot `PackedScene.get_state()`), backed by the existing node-template and
// connection tables.
// ───────────────────────────────────────────────────────────────────────────

/// Read-only view of a [`PackedScene`]'s node table and signal connections,
/// mirroring Godot's `SceneState`. Obtained via [`PackedScene::get_state`].
pub struct SceneState<'a> {
    templates: &'a [NodeTemplate],
    connections: &'a [SceneConnection],
}

impl PackedScene {
    /// Godot `PackedScene.get_state()` — a read-only [`SceneState`] for
    /// introspecting the packed node table and connections.
    pub fn get_state(&self) -> SceneState<'_> {
        SceneState {
            templates: &self.templates,
            connections: &self.connections,
        }
    }
}

impl<'a> SceneState<'a> {
    /// Number of nodes in the packed scene (`SceneState.get_node_count`).
    pub fn get_node_count(&self) -> usize {
        self.templates.len()
    }

    /// The node's name at `idx` (`SceneState.get_node_name`).
    pub fn get_node_name(&self, idx: usize) -> Option<&str> {
        self.templates.get(idx).map(|t| t.name.as_str())
    }

    /// The node's class/type at `idx` (`SceneState.get_node_type`).
    pub fn get_node_type(&self, idx: usize) -> Option<&str> {
        self.templates.get(idx).map(|t| t.class_name.as_str())
    }

    /// The groups the node at `idx` belongs to (`SceneState.get_node_groups`).
    pub fn get_node_groups(&self, idx: usize) -> &[String] {
        self.templates
            .get(idx)
            .map(|t| t.groups.as_slice())
            .unwrap_or_default()
    }

    /// Number of signal connections (`SceneState.get_connection_count`).
    pub fn get_connection_count(&self) -> usize {
        self.connections.len()
    }

    /// The emitting node path of connection `idx` (`get_connection_source`).
    pub fn get_connection_source(&self, idx: usize) -> Option<&str> {
        self.connections.get(idx).map(|c| c.from_path.as_str())
    }

    /// The signal name of connection `idx` (`get_connection_signal`).
    pub fn get_connection_signal(&self, idx: usize) -> Option<&str> {
        self.connections.get(idx).map(|c| c.signal_name.as_str())
    }

    /// The receiving node path of connection `idx` (`get_connection_target`).
    pub fn get_connection_target(&self, idx: usize) -> Option<&str> {
        self.connections.get(idx).map(|c| c.to_path.as_str())
    }

    /// The target method of connection `idx` (`get_connection_method`).
    pub fn get_connection_method(&self, idx: usize) -> Option<&str> {
        self.connections.get(idx).map(|c| c.method_name.as_str())
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
    fn mesh_sub_resource_resolved_to_primitive() {
        let tscn = r#"[gd_scene format=3]

[sub_resource type="SphereMesh" id="Sphere_1"]

[node name="World" type="Node3D"]

[node name="Ball" type="MeshInstance3D" parent="."]
mesh = SubResource("Sphere_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let ball = nodes
            .iter()
            .find(|n| n.name() == "Ball")
            .expect("Ball node");
        // `mesh = SubResource("id")` keeps its raw value; the resolved primitive
        // marker goes to the `__viewport_mesh__` companion for the viewport.
        assert_eq!(
            ball.get_property("mesh"),
            Variant::String("SubResource:Sphere_1".into())
        );
        assert_eq!(
            ball.get_property("__viewport_mesh__"),
            Variant::String("primitive:SphereMesh".into())
        );
    }

    #[test]
    fn shape_sub_resource_resolved_with_extents() {
        let tscn = r#"[gd_scene format=3]

[sub_resource type="BoxShape3D" id="Box_1"]
size = Vector3(2, 1, 3)

[sub_resource type="SphereShape3D" id="Sph_1"]
radius = 0.75

[node name="World" type="Node3D"]

[node name="BoxCol" type="CollisionShape3D" parent="."]
shape = SubResource("Box_1")

[node name="SphCol" type="CollisionShape3D" parent="."]
shape = SubResource("Sph_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let vp = |n: &str| -> Variant {
            nodes
                .iter()
                .find(|x| x.name() == n)
                .unwrap()
                .get_property("__viewport_shape__")
        };
        let raw = |n: &str| -> Variant {
            nodes
                .iter()
                .find(|x| x.name() == n)
                .unwrap()
                .get_property("shape")
        };
        // `shape = SubResource("id")` keeps its raw value; the resolved shape
        // marker (type + real extents) goes to the `__viewport_shape__` companion.
        assert_eq!(raw("BoxCol"), Variant::String("SubResource:Box_1".into()));
        assert_eq!(
            vp("BoxCol"),
            Variant::String("shape:BoxShape3D|2,1,3".into())
        );
        assert_eq!(
            vp("SphCol"),
            Variant::String("shape:SphereShape3D|0.75".into())
        );
    }

    /// pat-s3rrv.30 — a plain `StandardMaterial3D` reached through
    /// `GeometryInstance3D.material_override` must flatten onto the node.
    ///
    /// This slot matched NEITHER flattening pass: the `__material__*` pass read
    /// only `surface_material_override/0` and the mesh's own material, while the
    /// ShaderMaterial pass accepts only sub-resources whose class is literally
    /// `ShaderMaterial`. So the most obvious way to give a mesh a material in
    /// the Godot editor produced a node with no material companions at all, and
    /// the mesh rendered unmaterialed.
    ///
    /// ⚠️ The assertion deliberately checks a `__material__*` companion and not
    /// just `albedo_color`. `albedo_color` is special-cased and surfaced on its
    /// own, so a fix that resolved the id far enough to copy the colour but not
    /// the property bag would pass a colour-only assertion while every render
    /// mode, metallic and roughness value stayed dropped — which is the actual
    /// symptom this bead is about.
    #[test]
    fn material_override_flattens_a_plain_standard_material() {
        let tscn = r#"[gd_scene format=3]

[sub_resource type="BoxMesh" id="Box_1"]

[sub_resource type="StandardMaterial3D" id="Mat_1"]
albedo_color = Color(0, 0, 1, 1)
transparency = 1
cull_mode = 1

[node name="World" type="Node3D"]

[node name="BlueBox" type="MeshInstance3D" parent="."]
mesh = SubResource("Box_1")
material_override = SubResource("Mat_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let node = nodes.iter().find(|n| n.name() == "BlueBox").unwrap();

        assert_eq!(
            node.get_property("__material__class"),
            Variant::String("StandardMaterial3D".into()),
            "material_override must be resolved by the __material__ pass; the \
             ShaderMaterial pass only accepts class == ShaderMaterial, so a \
             plain material here matches neither and the mesh renders unmaterialed",
        );
        // The full bag, not just the colour — see the doc comment.
        assert_eq!(
            node.get_property("__material__transparency"),
            Variant::Int(1),
            "the material's render modes must travel on the node, not only its albedo",
        );
        assert_eq!(node.get_property("__material__cull_mode"), Variant::Int(1));
        match node.get_property("albedo_color") {
            Variant::Color(c) => assert!(
                c.r.abs() < 1e-4 && c.g.abs() < 1e-4 && (c.b - 1.0).abs() < 1e-4,
                "blue albedo: {c:?}"
            ),
            other => panic!("expected albedo_color Color, got {other:?}"),
        }
    }

    /// `material_override` outranks the per-surface slot.
    ///
    /// Godot's renderer picks
    /// `material_override.is_valid() ? material_override : p_material`
    /// (`render_forward_clustered.cpp:4206`), and the docs say the override is
    /// used "instead of any material set in any material slot of the mesh"
    /// (doc/classes/GeometryInstance3D.xml:67).
    ///
    /// ⚠️ Both materials are present on purpose. Appending `material_override`
    /// to the END of the resolve chain would satisfy the test above — the node
    /// gets a bag either way — and still pick the wrong material whenever a mesh
    /// carries both, which is exactly when the override exists to matter.
    #[test]
    fn material_override_wins_over_the_per_surface_slot() {
        let tscn = r#"[gd_scene format=3]

[sub_resource type="BoxMesh" id="Box_1"]

[sub_resource type="StandardMaterial3D" id="Slot_1"]
albedo_color = Color(1, 0, 0, 1)

[sub_resource type="StandardMaterial3D" id="Over_1"]
albedo_color = Color(0, 1, 0, 1)

[node name="World" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
mesh = SubResource("Box_1")
surface_material_override/0 = SubResource("Slot_1")
material_override = SubResource("Over_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let node = nodes.iter().find(|n| n.name() == "Box").unwrap();

        match node.get_property("albedo_color") {
            Variant::Color(c) => assert!(
                c.r.abs() < 1e-4 && (c.g - 1.0).abs() < 1e-4 && c.b.abs() < 1e-4,
                "the override (green) must win over the per-surface slot (red), got {c:?}",
            ),
            other => panic!("expected albedo_color Color, got {other:?}"),
        }
    }

    /// A ShaderMaterial override must stay out of the `__material__*` channel.
    ///
    /// ⚠️ A GUARD, not a discriminator — it held before `material_override`
    /// joined the resolve chain too. It is here because the obvious way to add
    /// that slot is without a class filter, and then a shader mesh would gain a
    /// `__material__*` bag containing none of the StandardMaterial3D keys:
    /// `material3d_for_node` would answer `Some(default)` instead of `None` and
    /// hand the renderer a flat default material for every shader-material mesh
    /// in the project.
    #[test]
    fn a_shader_material_override_does_not_gain_a_standard_material_bag() {
        let tscn = r#"[gd_scene format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "shader_type spatial; void fragment() { ALBEDO = vec3(1.0); }"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")

[node name="Water" type="MeshInstance3D"]
material_override = SubResource("ShaderMaterial_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let water = &nodes[0];

        assert!(
            !water.has_property("__material__class"),
            "a ShaderMaterial override belongs to the shader pass only; giving it \
             a __material__ bag makes material3d_for_node return a DEFAULT \
             material instead of None",
        );
        // …and the shader pass still owns it.
        match water.get_property("shader_code") {
            Variant::String(s) => assert!(s.contains("shader_type spatial"), "code: {s}"),
            other => panic!("expected shader_code string, got {other:?}"),
        }
    }

    #[test]
    fn mesh_material_albedo_resolved() {
        let tscn = r#"[gd_scene format=3]

[sub_resource type="BoxMesh" id="Box_1"]

[sub_resource type="StandardMaterial3D" id="Mat_1"]
albedo_color = Color(1, 0, 0, 1)

[node name="World" type="Node3D"]

[node name="RedBox" type="MeshInstance3D" parent="."]
mesh = SubResource("Box_1")
surface_material_override/0 = SubResource("Mat_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let red = nodes.iter().find(|n| n.name() == "RedBox").unwrap();
        // The material override's albedo is surfaced as an `albedo_color` Color.
        match red.get_property("albedo_color") {
            Variant::Color(c) => assert!(
                (c.r - 1.0).abs() < 1e-4 && c.g.abs() < 1e-4 && c.b.abs() < 1e-4,
                "red albedo: {c:?}"
            ),
            other => panic!("expected albedo_color Color, got {other:?}"),
        }
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

    // -----------------------------------------------------------------------
    // Sub-scene instancing (pat-05f)
    // -----------------------------------------------------------------------

    const PLAYER_TSCN: &str = r#"
[gd_scene format=3]

[node name="PlayerBody" type="Node2D"]
speed = 100

[node name="Sprite" type="Sprite2D" parent="."]
frame = 0

[node name="Hitbox" type="Area2D" parent="."]
"#;

    const MAIN_WITH_INSTANCE: &str = r#"
[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="1_player"]

[node name="World" type="Node"]

[node name="Player" parent="." instance=ExtResource("1_player")]
position = Vector2(100, 200)
"#;

    fn test_scene_resolver(path: &str) -> Option<PackedScene> {
        match path {
            "res://player.tscn" => Some(PackedScene::from_tscn(PLAYER_TSCN).unwrap()),
            _ => None,
        }
    }

    #[test]
    fn subscene_basic_instancing() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        // World + Player(PlayerBody renamed) + Sprite + Hitbox = 4 nodes
        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].name(), "World");
        assert_eq!(nodes[1].name(), "Player"); // renamed from PlayerBody
        assert_eq!(nodes[2].name(), "Sprite");
        assert_eq!(nodes[3].name(), "Hitbox");
    }

    #[test]
    fn subscene_class_names_from_subscene() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        assert_eq!(nodes[1].class_name(), "Node2D"); // from player.tscn
        assert_eq!(nodes[2].class_name(), "Sprite2D");
        assert_eq!(nodes[3].class_name(), "Area2D");
    }

    #[test]
    fn subscene_property_overrides_applied() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        // position override from main scene
        assert_eq!(
            nodes[1].get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );
        // speed from the sub-scene's own template
        assert_eq!(nodes[1].get_property("speed"), Variant::Int(100));
    }

    #[test]
    fn subscene_original_properties_preserved() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        // Sprite's frame property from sub-scene
        assert_eq!(nodes[2].get_property("frame"), Variant::Int(0));
    }

    #[test]
    fn subscene_hierarchy_correct() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        // Player is child of World
        assert_eq!(nodes[1].parent(), Some(nodes[0].id()));
        // Sprite is child of Player
        assert_eq!(nodes[2].parent(), Some(nodes[1].id()));
        // Hitbox is child of Player
        assert_eq!(nodes[3].parent(), Some(nodes[1].id()));
        // World's children should include Player
        assert!(nodes[0].children().contains(&nodes[1].id()));
    }

    #[test]
    fn subscene_add_to_tree() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();

        let scene_root_id =
            add_packed_scene_to_tree_with_subscenes(&mut tree, root, &scene, &test_scene_resolver)
                .unwrap();

        // root + World + Player + Sprite + Hitbox = 5
        assert_eq!(tree.node_count(), 5);
        assert_eq!(tree.node_path(scene_root_id).unwrap(), "/root/World");

        let player = tree.get_node_by_path("/root/World/Player").unwrap();
        let player_node = tree.get_node(player).unwrap();
        assert_eq!(player_node.class_name(), "Node2D");
        assert_eq!(
            player_node.get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 200.0))
        );

        let sprite = tree.get_node_by_path("/root/World/Player/Sprite").unwrap();
        assert!(tree.get_node(sprite).is_some());

        let hitbox = tree.get_node_by_path("/root/World/Player/Hitbox").unwrap();
        assert!(tree.get_node(hitbox).is_some());
    }

    #[test]
    fn subscene_unresolvable_falls_through() {
        // When the resolver returns None, the node should be a regular
        // node with the _instance property set (fallback behavior).
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&|_| None).unwrap();

        // World + Player (as regular node) = 2 nodes
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1].name(), "Player");
        // Should have _instance property since it couldn't be resolved
        assert_eq!(
            nodes[1].get_property("_instance"),
            Variant::String("ExtResource(\"1_player\")".into())
        );
    }

    #[test]
    fn subscene_nested_two_levels() {
        let weapon_tscn = r#"
[gd_scene format=3]

[node name="Weapon" type="Node2D"]
damage = 10

[node name="Particle" type="Node2D" parent="."]
"#;

        let player_with_weapon = r#"
[gd_scene format=3]

[ext_resource type="PackedScene" path="res://weapon.tscn" id="1_weapon"]

[node name="PlayerBody" type="Node2D"]

[node name="Sword" parent="." instance=ExtResource("1_weapon")]
damage = 25
"#;

        let main = r#"
[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player_armed.tscn" id="1_p"]

[node name="World" type="Node"]

[node name="Player" parent="." instance=ExtResource("1_p")]
"#;

        let resolver = |path: &str| -> Option<PackedScene> {
            match path {
                "res://weapon.tscn" => Some(PackedScene::from_tscn(weapon_tscn).unwrap()),
                "res://player_armed.tscn" => {
                    Some(PackedScene::from_tscn(player_with_weapon).unwrap())
                }
                _ => None,
            }
        };

        let scene = PackedScene::from_tscn(main).unwrap();
        let nodes = scene.instance_with_subscenes(&resolver).unwrap();

        // World + Player(PlayerBody) + Sword(Weapon) + Particle = 4 nodes
        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].name(), "World");
        assert_eq!(nodes[1].name(), "Player");
        assert_eq!(nodes[2].name(), "Sword");
        assert_eq!(nodes[3].name(), "Particle");

        // Nested override: damage overridden to 25
        assert_eq!(nodes[2].get_property("damage"), Variant::Int(25));
    }

    #[test]
    fn subscene_multiple_instances_same_scene() {
        let tscn = r#"
[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="1_player"]

[node name="World" type="Node"]

[node name="Player1" parent="." instance=ExtResource("1_player")]
position = Vector2(100, 0)

[node name="Player2" parent="." instance=ExtResource("1_player")]
position = Vector2(200, 0)
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        // World + Player1(+Sprite+Hitbox) + Player2(+Sprite+Hitbox) = 7
        assert_eq!(nodes.len(), 7);
        assert_eq!(nodes[1].name(), "Player1");
        assert_eq!(nodes[4].name(), "Player2");
        assert_eq!(
            nodes[1].get_property("position"),
            Variant::Vector2(Vector2::new(100.0, 0.0))
        );
        assert_eq!(
            nodes[4].get_property("position"),
            Variant::Vector2(Vector2::new(200.0, 0.0))
        );
    }

    #[test]
    fn subscene_owner_is_the_instance_root_not_the_main_scene() {
        // Godot: a node is owned by the root of the scene that DECLARED it. The
        // instance root (Player) belongs to World; Player's own Sprite/Hitbox
        // belong to Player — which is what hides them in the editor's tree and
        // scopes their `%Unique` names. This test used to assert every node was
        // owned by World, the flat list's old (wrong) answer.
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        let world = nodes[0].id();
        let player = nodes[1].id();
        assert!(nodes[0].owner().is_none(), "the scene root has no owner");
        assert_eq!(nodes[1].owner(), Some(world), "the instance root belongs to World");
        for node in &nodes[2..] {
            assert_eq!(
                node.owner(),
                Some(player),
                "{} was declared by player.tscn, so Player owns it",
                node.name()
            );
        }
    }

    #[test]
    fn subscene_mixed_regular_and_instanced_nodes() {
        let tscn = r#"
[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="1_player"]

[node name="World" type="Node"]

[node name="Camera" type="Camera2D" parent="."]

[node name="Player" parent="." instance=ExtResource("1_player")]

[node name="HUD" type="Control" parent="."]
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        // World + Camera + Player(+Sprite+Hitbox) + HUD = 6
        assert_eq!(nodes.len(), 6);
        assert_eq!(nodes[0].name(), "World");
        assert_eq!(nodes[1].name(), "Camera");
        assert_eq!(nodes[1].class_name(), "Camera2D");
        assert_eq!(nodes[2].name(), "Player");
        assert_eq!(nodes[3].name(), "Sprite");
        assert_eq!(nodes[4].name(), "Hitbox");
        assert_eq!(nodes[5].name(), "HUD");
        assert_eq!(nodes[5].class_name(), "Control");
    }

    #[test]
    fn resolve_ext_resource_path_works() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        assert_eq!(
            scene.resolve_ext_resource_path("ExtResource(\"1_player\")"),
            Some("res://player.tscn")
        );
        assert_eq!(
            scene.resolve_ext_resource_path("ExtResource(\"missing\")"),
            None
        );
        assert_eq!(scene.resolve_ext_resource_path("not_a_ref"), None);
    }

    #[test]
    fn ext_resources_accessor() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let ext = scene.ext_resources();
        assert_eq!(ext.len(), 1);
        let entry = ext.get("1_player").unwrap();
        assert_eq!(entry.res_type, "PackedScene");
        assert_eq!(entry.path, "res://player.tscn");
    }

    // -- Camera3D current auto-assignment tests ---------------------------------

    const CAMERA3D_SCENE: &str = r#"
[gd_scene format=3 uid="uid://cam3d"]

[node name="World" type="Node3D"]

[node name="Camera" type="Camera3D" parent="."]
transform = Transform3D(1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 2, 8)
fov = 70.0

[node name="Mesh" type="MeshInstance3D" parent="."]
"#;

    #[test]
    fn auto_assign_camera_current_on_load() {
        let scene = PackedScene::from_tscn(CAMERA3D_SCENE).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        // Find the Camera3D node.
        let mut subtree = Vec::new();
        tree.collect_subtree_top_down(scene_root, &mut subtree);
        let cam_id = subtree
            .iter()
            .find(|&&nid| {
                tree.get_node(nid)
                    .map(|n| n.class_name() == "Camera3D")
                    .unwrap_or(false)
            })
            .copied()
            .expect("Camera3D should exist");

        // Camera should have been auto-assigned current=true.
        assert_eq!(
            tree.get_node(cam_id).unwrap().get_property("current"),
            Variant::Bool(true),
            "first Camera3D should be auto-assigned current=true"
        );
    }

    const CAMERA3D_EXPLICIT_CURRENT: &str = r#"
[gd_scene format=3 uid="uid://cam3dc"]

[node name="World" type="Node3D"]

[node name="CamA" type="Camera3D" parent="."]
fov = 60.0

[node name="CamB" type="Camera3D" parent="."]
current = true
fov = 90.0
"#;

    #[test]
    fn explicit_current_not_overridden() {
        let scene = PackedScene::from_tscn(CAMERA3D_EXPLICIT_CURRENT).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();

        let mut subtree = Vec::new();
        tree.collect_subtree_top_down(scene_root, &mut subtree);

        let cam_a = subtree
            .iter()
            .find(|&&nid| {
                tree.get_node(nid)
                    .map(|n| n.name() == "CamA")
                    .unwrap_or(false)
            })
            .copied()
            .expect("CamA");
        let cam_b = subtree
            .iter()
            .find(|&&nid| {
                tree.get_node(nid)
                    .map(|n| n.name() == "CamB")
                    .unwrap_or(false)
            })
            .copied()
            .expect("CamB");

        // CamB has explicit current=true; CamA should NOT have been auto-assigned.
        assert_eq!(
            tree.get_node(cam_b).unwrap().get_property("current"),
            Variant::Bool(true),
            "CamB should keep explicit current=true"
        );
        assert_ne!(
            tree.get_node(cam_a).unwrap().get_property("current"),
            Variant::Bool(true),
            "CamA should not be auto-assigned when CamB is already current"
        );
    }

    #[test]
    fn no_camera_no_panic() {
        // Scene with no Camera3D — auto_assign should be a no-op.
        let scene = PackedScene::from_tscn(SIMPLE_TSCN).unwrap();
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let _scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
        // Just verify it didn't panic.
    }

    // -- ShaderMaterial flattening (pat-u3l8c.43) ---------------------------

    /// A ShaderMaterial sub-resource whose `shader` is an inline
    /// `[sub_resource type="Shader"]` (with `code`) flattens its source,
    /// shader_type, and shader_parameter/* onto the referencing node.
    #[test]
    fn shader_material_inline_shader_flattens_onto_node() {
        let tscn = r#"
[gd_scene load_steps=3 format=3]

[sub_resource type="Shader" id="Shader_1"]
code = "shader_type spatial; uniform float wave_height = 0.5; void fragment() { ALBEDO = vec3(1.0); }"

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = SubResource("Shader_1")
shader_parameter/wave_height = 0.75
shader_parameter/tint = Color(0.2, 0.4, 0.8, 1)

[node name="Water" type="MeshInstance3D"]
material_override = SubResource("ShaderMaterial_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let water = &nodes[0];
        assert_eq!(water.name(), "Water");

        // Shader source flattened onto the node.
        match water.get_property("shader_code") {
            Variant::String(s) => assert!(s.contains("shader_type spatial"), "code: {s}"),
            other => panic!("expected shader_code string, got {other:?}"),
        }
        assert_eq!(
            water.get_property("shader_type"),
            Variant::String("spatial".into())
        );
        // Parameters copied verbatim.
        assert_eq!(
            water.get_property("shader_parameter/wave_height"),
            Variant::Float(0.75)
        );
        assert_eq!(
            water.get_property("shader_parameter/tint"),
            Variant::Color(gdcore::math::Color::new(0.2, 0.4, 0.8, 1.0))
        );
    }

    /// A ShaderMaterial whose `shader` is an external `.gdshader` ext_resource
    /// records the `.gdshader` path under `shader_path` plus the parameters.
    #[test]
    fn shader_material_ext_shader_records_path() {
        let tscn = r#"
[gd_scene load_steps=3 format=3]

[ext_resource type="Shader" path="res://shaders/water.gdshader" id="1_shd"]

[sub_resource type="ShaderMaterial" id="ShaderMaterial_1"]
shader = ExtResource("1_shd")
shader_parameter/speed = 2.0

[node name="Ocean" type="MeshInstance3D"]
material_override = SubResource("ShaderMaterial_1")
"#;
        let scene = PackedScene::from_tscn(tscn).unwrap();
        let nodes = scene.instance().unwrap();
        let ocean = &nodes[0];
        assert_eq!(
            ocean.get_property("shader_path"),
            Variant::String("res://shaders/water.gdshader".into())
        );
        assert_eq!(
            ocean.get_property("shader_parameter/speed"),
            Variant::Float(2.0)
        );
        // No inline code in the ext case.
        assert_eq!(ocean.get_property("shader_code"), Variant::Nil);
    }

    #[test]
    fn parses_connection_binds_editable_and_placeholder() {
        let tscn = r#"[gd_scene load_steps=1 format=3]

[node name="Root" type="Node2D"]

[node name="Button" type="Button" parent="."]

[node name="Sub" parent="." instance_placeholder="res://sub.tscn"]

[editable path="Child/Instance"]

[connection signal="pressed" from="Button" to="." method="_on_pressed" binds=[42, "hi"]]
"#;
        let scene = PackedScene::from_tscn(tscn).expect("parse");

        // Connection `binds=[...]` captured (previously dropped).
        let conns = scene.connections();
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].method_name, "_on_pressed");
        assert_eq!(
            conns[0].binds,
            vec![Variant::Int(42), Variant::String("hi".into())]
        );

        // `[editable path="..."]` section captured (previously ignored).
        assert_eq!(scene.editable_instances(), &["Child/Instance".to_string()]);

        // `instance_placeholder="..."` captured (previously unhandled).
        assert_eq!(
            scene.placeholder_nodes(),
            vec![("Sub".to_string(), "res://sub.tscn".to_string())]
        );
    }

    #[test]
    fn connection_without_binds_and_no_editable_or_placeholder() {
        let tscn = r#"[gd_scene format=3]

[node name="Root" type="Node"]

[node name="Child" type="Node" parent="."]

[connection signal="ready" from="." to="." method="_go"]
"#;
        let scene = PackedScene::from_tscn(tscn).expect("parse");
        assert_eq!(scene.connections().len(), 1);
        assert!(scene.connections()[0].binds.is_empty());
        assert!(scene.editable_instances().is_empty());
        assert!(scene.placeholder_nodes().is_empty());
    }

    // -- ExtResource path resolution (pat-0q2kx.6) ---------------------------

    /// Every `prop = ExtResource("id")` gains an `__extres__<prop>` companion
    /// holding the path from its `[ext_resource]` header, while the raw ref
    /// stays on the property itself. Before this, only `script` resolved, so a
    /// scene's `stream` / `texture` / `mesh` reference was an unusable literal.
    #[test]
    fn ext_resource_properties_resolve_to_their_paths() {
        let tscn = r#"[gd_scene load_steps=3 format=3]

[ext_resource type="AudioStreamWAV" path="res://sfx/beep.wav" id="1_beep"]
[ext_resource type="Texture2D" path="res://art/icon.png" id="2_icon"]

[node name="World" type="Node"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_beep")
autoplay = true

[node name="Icon" type="Sprite2D" parent="."]
texture = ExtResource("2_icon")
"#;
        let scene = PackedScene::from_tscn(tscn).expect("parse");
        let nodes = scene.instance().expect("instance");

        let beeper = nodes.iter().find(|n| n.name() == "Beeper").expect("Beeper");
        assert_eq!(
            beeper.get_property("__extres__stream"),
            Variant::String("res://sfx/beep.wav".into())
        );
        // The raw reference is preserved for round-trip and the inspector.
        assert_eq!(
            beeper.get_property("stream"),
            Variant::String("ExtResource(\"1_beep\")".into())
        );
        // Non-reference properties are untouched.
        assert_eq!(beeper.get_property("autoplay"), Variant::Bool(true));

        let icon = nodes.iter().find(|n| n.name() == "Icon").expect("Icon");
        assert_eq!(
            icon.get_property("__extres__texture"),
            Variant::String("res://art/icon.png".into())
        );
    }

    /// A reference to an id with no `[ext_resource]` header resolves to
    /// nothing rather than inventing a path.
    #[test]
    fn ext_resource_ref_to_unknown_id_is_not_resolved() {
        let tscn = r#"[gd_scene format=3]

[node name="World" type="Node"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = ExtResource("nope")
"#;
        let scene = PackedScene::from_tscn(tscn).expect("parse");
        let nodes = scene.instance().expect("instance");
        let beeper = nodes.iter().find(|n| n.name() == "Beeper").expect("Beeper");
        assert_eq!(beeper.get_property("__extres__stream"), Variant::Nil);
    }

    /// The companion is internal: it must never be written back out as a scene
    /// property, or a save would invent a key Godot does not know.
    #[test]
    fn extres_companion_is_not_saved_back_to_tscn() {
        let tscn = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="AudioStreamWAV" path="res://sfx/beep.wav" id="1_beep"]

[node name="World" type="Node"]

[node name="Beeper" type="AudioStreamPlayer" parent="."]
stream = ExtResource("1_beep")
"#;
        let scene = PackedScene::from_tscn(tscn).expect("parse");
        let mut tree = SceneTree::new();
        let root = tree.root_id();
        let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance");

        let saved = crate::scene_saver::TscnSaver::save_tree(&tree, scene_root);
        assert!(
            !saved.contains("__extres__"),
            "internal companion leaked into the saved scene:\n{saved}"
        );
    }

    #[test]
    fn parse_ext_resource_ref_accepts_both_spellings() {
        // The `.tscn` call form this parser stores...
        assert_eq!(
            parse_ext_resource_ref("ExtResource(\"1_abc\")").as_deref(),
            Some("1_abc")
        );
        // ...and the form `gdresource`'s value parser produces.
        assert_eq!(
            parse_ext_resource_ref("ExtResource:1_abc").as_deref(),
            Some("1_abc")
        );
        assert_eq!(parse_ext_resource_ref("res://plain/path.wav"), None);
        assert_eq!(parse_ext_resource_ref("SubResource(\"1\")"), None);
        assert_eq!(parse_ext_resource_ref("ExtResource()"), None);
    }
}

/// Number of baked samples for a `width_curve`, matching Godot's default
/// `Curve.bake_resolution` of 100 plus the closing sample.
const WIDTH_CURVE_BAKE: usize = 101;

/// Reads a `Variant` that Godot may have written as either a float or an int.
fn curve_scalar(v: &Variant) -> f32 {
    match v {
        Variant::Float(f) => *f as f32,
        Variant::Int(i) => *i as f32,
        _ => 0.0,
    }
}

/// Scalar cubic Bézier, mirroring `Math::bezier_interpolate`
/// (upstream/godot/core/math/math_funcs.h).
fn bezier_scalar(start: f32, c1: f32, c2: f32, end: f32, t: f32) -> f32 {
    let omt = 1.0 - t;
    let omt2 = omt * omt;
    let omt3 = omt2 * omt;
    let t2 = t * t;
    let t3 = t2 * t;
    start * omt3 + c1 * omt2 * t * 3.0 + c2 * omt * t2 * 3.0 + end * t3
}

/// Bakes a Godot `Curve` sub-resource's `_data` into a dense sample table over
/// the curve's `[0, 1]` domain.
///
/// `_data` is a flat array carrying FIVE elements per point — position, left
/// tangent, right tangent, left mode, right mode (`Curve::get_data`,
/// upstream/godot/scene/resources/curve.cpp:440). Only the position and the two
/// tangents affect the sampled value; the modes are editor state.
///
/// ⚠️ Godot samples BETWEEN two points as a cubic Bézier whose inner control
/// points sit a third of the span apart, offset by the tangents
/// (`Curve::sample_local_nocheck`, curve.cpp:475). That is **not** a straight
/// line: with the tangents left at zero — the common authored case — the ramp
/// is smoothstep-shaped, so interpolating linearly would visibly disagree with
/// Godot at every interior sample while still passing a monotone "thicker at one
/// end" check.
///
/// Line2D reads its curve through `sample_baked`, so baking here is what Godot
/// itself does rather than an approximation of it. Returns `None` for a bag
/// whose `_data` is absent, empty, or not a whole number of points.
/// ⚠️ `pub(crate)` as of pat-mgixk.82: the state-machine loader bakes a
/// transition's `xfade_curve` through this same function. Widening the
/// visibility rather than copying it keeps ONE implementation of Godot's
/// `Curve::sample` in the crate — the cubic Bezier below is easy to get subtly
/// wrong, and two copies would drift.
pub(crate) fn bake_curve_samples(data: &[Variant]) -> Option<Vec<f32>> {
    const ELEMS: usize = 5;
    if data.is_empty() || data.len() % ELEMS != 0 {
        return None;
    }
    struct CurvePt {
        pos: gdcore::math::Vector2,
        left_tangent: f32,
        right_tangent: f32,
    }
    let mut points = Vec::with_capacity(data.len() / ELEMS);
    for chunk in data.chunks_exact(ELEMS) {
        let Variant::Vector2(pos) = &chunk[0] else {
            // A malformed point makes every later index wrong, so bail rather
            // than bake a table that is silently misaligned.
            return None;
        };
        points.push(CurvePt {
            pos: *pos,
            left_tangent: curve_scalar(&chunk[1]),
            right_tangent: curve_scalar(&chunk[2]),
        });
    }

    let sample = |offset: f32| -> f32 {
        if points.len() == 1 {
            return points[0].pos.y;
        }
        // Godot's `get_index`: the last point at or before `offset`.
        let mut i = 0;
        while i + 1 < points.len() && points[i + 1].pos.x <= offset {
            i += 1;
        }
        if i == points.len() - 1 {
            return points[i].pos.y;
        }
        let local = offset - points[i].pos.x;
        if i == 0 && local <= 0.0 {
            return points[0].pos.y;
        }
        let a = &points[i];
        let b = &points[i + 1];
        let mut d = b.pos.x - a.pos.x;
        if d.abs() < 1e-6 {
            return b.pos.y;
        }
        let t = local / d;
        d /= 3.0;
        let yac = a.pos.y + d * a.right_tangent;
        let ybc = b.pos.y - d * b.left_tangent;
        bezier_scalar(a.pos.y, yac, ybc, b.pos.y, t)
    };

    let last = (WIDTH_CURVE_BAKE - 1) as f32;
    Some(
        (0..WIDTH_CURVE_BAKE)
            .map(|i| sample(i as f32 / last))
            .collect(),
    )
}
