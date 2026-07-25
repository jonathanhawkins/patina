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
}

/// An external resource reference parsed from `[ext_resource]` sections.
#[derive(Debug, Clone)]
pub struct ExtResourceEntry {
    /// The resource type (e.g. `"Script"`, `"PackedScene"`, `"Texture2D"`).
    pub res_type: String,
    /// The resource path (e.g. `"res://scripts/player.gd"`).
    pub path: String,
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

        for (line_num, line) in source.lines().enumerate() {
            let _line_num = line_num + 1; // 1-based, for error reporting
            let line = line.trim();

            // Continue accumulating a multi-line property value from a prior line.
            if let Some((key, mut acc)) = pending_prop.take() {
                if !acc.is_empty() {
                    acc.push(' ');
                }
                acc.push_str(line);
                if bracket_depth(&acc) > 0 {
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
                                binds,
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
                    });
                } else if inner.starts_with("ext_resource") {
                    let attrs = extract_header_attrs(inner);
                    if let (Some(id), Some(path)) =
                        (attrs.get("id").cloned(), attrs.get("path").cloned())
                    {
                        let res_type = attrs.get("type").cloned().unwrap_or_default();
                        ext_resources.insert(id, ExtResourceEntry { res_type, path });
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
                        }
                        // Store the raw reference as a string property too.
                        tmpl.properties
                            .insert(key.to_string(), Variant::String(value_str.to_string()));
                        continue;
                    }

                    // A value that opens more brackets/braces than it closes on
                    // this line continues onto following lines (e.g. a GridMap
                    // `data = {` dictionary). Buffer it until the brackets balance.
                    if bracket_depth(value_str) > 0 {
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
                    if bracket_depth(value_str) > 0 {
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
            templates.push(tmpl);
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
                tmpl.properties
                    .insert(format!("__resref__{prop}"), Variant::String(meta));
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
        for tmpl in &mut templates {
            // Godot's resolve order: a node-level override wins over the mesh's
            // own material (`material_override` is handled by the ShaderMaterial
            // pass below).
            let mat_id =
                sub_ref_id(tmpl.properties.get("surface_material_override/0")).or_else(|| {
                    sub_ref_id(tmpl.properties.get("mesh"))
                        .and_then(|mesh_id| sub_resource_props.get(&mesh_id))
                        .and_then(|p| sub_ref_id(p.get("material")))
                });
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
            let resolved: Vec<(String, String)> = tmpl
                .properties
                .iter()
                .filter_map(|(k, v)| {
                    let s = match v {
                        Variant::String(s) => s,
                        _ => return None,
                    };
                    let id = parse_ext_resource_ref(s)?;
                    let entry = ext_resources.get(&id)?;
                    Some((format!("__extres__{k}"), entry.path.clone()))
                })
                .collect();
            for (key, path) in resolved {
                tmpl.properties.insert(key, Variant::String(path));
            }
        }

        // Resolve `mesh = SubResource("id")` references to a concrete primitive
        // marker (`primitive:<Type>`) the 3D viewport renderer understands, stored
        // under a `__viewport_mesh__` companion so the real `mesh` property keeps
        // its `SubResource:<id>` value (correct scene round-trip + inspector).
        for tmpl in &mut templates {
            let resolved = match tmpl.properties.get("mesh") {
                Some(Variant::String(s)) => s
                    .strip_prefix("SubResource:")
                    .and_then(|id| sub_resources.get(id))
                    .map(|ty| format!("primitive:{ty}")),
                _ => None,
            };
            if let Some(p) = resolved {
                tmpl.properties
                    .insert("__viewport_mesh__".into(), Variant::String(p));
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

        // Create root node (may itself be a sub-scene instance, but that's
        // handled by the caller).
        let mut root_node = Node::new(&root_tmpl.name, &root_tmpl.class_name);
        for (key, value) in &root_tmpl.properties {
            root_node.set_property(key, value.clone());
        }
        for group in &root_tmpl.groups {
            root_node.add_to_group(group.clone());
        }
        root_node.set_unique_name(root_tmpl.unique_name);
        if let Some(ref script_path) = root_tmpl.script_path {
            root_node.set_property("_script_path", Variant::String(script_path.clone()));
        }
        path_to_index.insert(".".into(), 0);
        path_to_index.insert(root_tmpl.name.clone(), 0);
        let root_id = root_node.id();
        nodes.push(root_node);

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
                        let sub_nodes = sub_scene.instance_with_subscenes(resolve_scene)?;
                        if sub_nodes.is_empty() {
                            continue;
                        }

                        // The sub-scene root replaces this template's node.
                        // Apply overrides: rename to template name, apply
                        // template properties on top.
                        let sub_root_idx = nodes.len();
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
                                for (key, value) in &tmpl.properties {
                                    sub_node.set_property(key, value.clone());
                                }
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

            // Regular (non-instanced) node.
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

            let node_path = if parent_path == "." {
                tmpl.name.clone()
            } else {
                format!("{}/{}", parent_path, tmpl.name)
            };
            path_to_index.insert(node_path, child_idx);

            node.set_parent(Some(nodes[parent_idx].id()));
            nodes[parent_idx].add_child_id(child_id);

            nodes.push(node);
        }

        Ok(nodes)
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

/// Net bracket depth of `s`: `([{` count as `+1`, `)]}` as `-1`, ignoring any
/// characters inside double-quoted strings. A positive result means the value
/// opens more containers than it closes, i.e. it continues on a later line
/// (used to stitch multi-line `.tscn` property values such as a GridMap `data`
/// dictionary back into a single parseable string).
fn bracket_depth(s: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for c in s.chars() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            match c {
                '\\' => escape = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth
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

    Ok(new_root_id)
}

/// Adds all nodes from a [`PackedScene::instance_with_subscenes()`] call
/// into a [`crate::SceneTree`] under the given parent, with recursive
/// sub-scene instancing.
///
/// The `resolve_scene` callback is invoked for each `instance=ExtResource`
/// reference, receiving the `res://` path and returning the parsed sub-scene.
pub fn add_packed_scene_to_tree_with_subscenes<F>(
    tree: &mut crate::scene_tree::SceneTree,
    parent_id: crate::node::NodeId,
    scene: &PackedScene,
    resolve_scene: &F,
) -> EngineResult<crate::node::NodeId>
where
    F: Fn(&str) -> Option<PackedScene>,
{
    let nodes = scene.instance_with_subscenes(resolve_scene)?;
    if nodes.is_empty() {
        return Err(EngineError::InvalidOperation(
            "instanced scene produced no nodes".into(),
        ));
    }

    let mut old_to_new: HashMap<crate::node::NodeId, crate::node::NodeId> = HashMap::new();

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
        new_node.set_owner(Some(new_root_id));
        let new_node_id = new_node.id();
        old_to_new.insert(node.id(), new_node_id);
        tree.add_child(new_parent_id, new_node)?;
    }

    wire_connections(tree, new_root_id, &scene.connections);

    crate::animation_tscn::attach_scene_animation_players(tree, new_root_id, scene);

    crate::animation_tree_tscn::attach_scene_animation_trees(tree, new_root_id, scene);

    auto_assign_current_camera(tree, new_root_id);

    Ok(new_root_id)
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
    fn subscene_owner_is_main_scene_root() {
        let scene = PackedScene::from_tscn(MAIN_WITH_INSTANCE).unwrap();
        let nodes = scene.instance_with_subscenes(&test_scene_resolver).unwrap();

        let root_id = nodes[0].id();
        assert!(nodes[0].owner().is_none()); // root has no owner
        for node in &nodes[1..] {
            assert_eq!(node.owner(), Some(root_id));
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
