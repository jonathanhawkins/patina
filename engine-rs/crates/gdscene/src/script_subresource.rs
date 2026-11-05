//! Script-visible inline sub-resources, backed by the node companions
//! (pat-nyn3w).
//!
//! # The problem
//!
//! A `.tscn` property such as `environment = SubResource("Environment_dauhq")`
//! is stored on the node as the TEXT `"SubResource:Environment_dauhq"`, with the
//! sub-resource's body flattened beside it as `__subres__<prop>__<key>`
//! companions (`packed_scene.rs`) — and, for a few properties, a specialised
//! mirror the renderers read (`__material__*`, `__shape__*`, `__curve__*`,
//! `shader_parameter/*`). Scripts were handed the text, so
//! `$WorldEnvironment.environment` printed `SubResource:Environment_dauhq` and
//! truck_town's speedometer raised `String.sample` every frame.
//!
//! # The design: ONE store, read as a view
//!
//! The companions are already what the renderers, the saver and the audio
//! bridge read, so they stay the single source of truth. A script reading
//! `node.<prop>` gets a `Variant::Resource` SNAPSHOT of them carrying a
//! [`BOUND_RESOURCE_KEY`] binding (`"<node id>|<property path>"`), and the
//! interpreter routes every member read and write on such a value back here:
//!
//! - [`bound_get`] reads one field off the companions as they are NOW, so a
//!   value captured in a variable sees later writes (Godot: one shared object);
//! - [`bound_set`] writes one field onto the companions of EVERY node holding
//!   the same sub-resource (`a.shape.height = 3` is seen through `b.shape` when
//!   both reference `CapsuleShape3D_pxdqy`), on the verbatim body AND on the
//!   specialised mirror its consumers read;
//! - nested sub-resources (`environment.sky`, `GradientTexture2D.gradient`) are
//!   flattened under a `:`-joined path (`__subres__environment:sky__*`), so
//!   `environment.sky.sky_material = preload(..)` lands too.
//!
//! The snapshot's `resource_path` is Godot's `"<scene>::<id>"` when the owning
//! scene's path is known.

use gdcore::id::ObjectId;
use gdscript_interop::bindings::BOUND_RESOURCE_KEY;
use gdvariant::{ResourceRef, Variant};

use crate::node::{Node, NodeId};
use crate::SceneTree;

/// How deep a nested sub-resource chain is snapshotted (the flattening in
/// `packed_scene` stops at the same depth).
const MAX_DEPTH: usize = 4;

/// The `[sub_resource]` meta (`"<class>::<id>"`) a property path names on
/// `node`: `__resref__<prop>` at the top level, the nested resref key below it.
fn meta_of(node: &Node, path: &str) -> Option<String> {
    let key = if path.contains(':') {
        crate::packed_scene::nested_resref_key(path)
    } else {
        format!("__resref__{path}")
    };
    match node.get_property(&key) {
        Variant::String(s) if s.contains("::") => Some(s),
        _ => None,
    }
}

/// The verbatim body prefix of a property path.
fn body_prefix(path: &str) -> String {
    format!("__subres__{path}__")
}

/// The specialised mirror its consumers read for a TOP-LEVEL property, when
/// the node actually carries one (`__material__*`, `__shape__*`, `__curve__*`).
fn mirror_prefix(node: &Node, path: &str) -> Option<String> {
    if path.contains(':') {
        return None;
    }
    let prefix = crate::scene_saver::companion_prefix_for(path)?;
    node.properties()
        .any(|(k, _)| k.starts_with(prefix.as_str()))
        .then_some(prefix)
}

/// Whether a ShaderMaterial's `shader_parameter/*` is mirrored straight onto
/// the node (`packed_scene` copies them there for the render server).
fn mirrors_shader_parameters(path: &str, field: &str) -> bool {
    field.starts_with("shader_parameter/")
        && matches!(
            path,
            "material" | "material_override" | "surface_material_override/0"
        )
}

thread_local! {
    /// The holder last used to read each binding. Any holder will do — writes
    /// keep every holder in step — so this only saves the scan, and every hit
    /// is re-validated before use.
    static HOLDER_CACHE: std::cell::RefCell<std::collections::HashMap<String, (NodeId, String)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The scope key a binding carries (see [`scope_of`]).
fn scope_key(tree: &SceneTree, id: NodeId) -> String {
    match scope_of(tree, id) {
        (Some(file), _) => format!("file:{file}"),
        (None, owner) => format!("node:{}", owner.raw()),
    }
}

/// The binding of the sub-resource at `path` on `id`: `"<meta>|<scope>"`.
///
/// 🔑 It names the RESOURCE, not the node it was read through, so
/// `is_same(a.shape, b.shape)` holds when both reference
/// `CapsuleShape3D_pxdqy`, as it does in Godot — a node-keyed binding made two
/// reads of one resource compare unequal.
fn binding_for(tree: &SceneTree, id: NodeId, path: &str) -> Option<String> {
    let meta = meta_of(tree.get_node(id)?, path)?;
    let binding = format!("{meta}|{}", scope_key(tree, id));
    HOLDER_CACHE.with(|c| {
        c.borrow_mut()
            .entry(binding.clone())
            .or_insert_with(|| (id, path.to_string()));
    });
    Some(binding)
}

/// A node and property path currently holding the sub-resource `binding`
/// names.
fn resolve(tree: &SceneTree, binding: &str) -> Option<(NodeId, String)> {
    let (meta, scope) = binding.split_once('|')?;
    let valid = |id: NodeId, path: &str| {
        tree.get_node(id)
            .and_then(|n| meta_of(n, path))
            .is_some_and(|m| m == meta)
            && scope_key(tree, id) == scope
    };
    if let Some((id, path)) = HOLDER_CACHE.with(|c| c.borrow().get(binding).cloned()) {
        if valid(id, &path) {
            return Some((id, path));
        }
    }
    let found = holders(tree, meta, scope).into_iter().next()?;
    HOLDER_CACHE.with(|c| c.borrow_mut().insert(binding.to_string(), found.clone()));
    Some(found)
}

/// The scope a node's sub-resource ids belong to: its owning scene instance's
/// file when known, else that instance's root id.
///
/// 🔑 A sub-resource id is SCENE-LOCAL (`SubResource("1")` recurs across
/// files), so sharing must never cross a scene. Within one, Godot shares a
/// non-local resource between every node that names it — and between instances
/// of the same `.tscn`, which the file key gives for an instanced sub-scene.
fn scope_of(tree: &SceneTree, id: NodeId) -> (Option<String>, NodeId) {
    let Some(node) = tree.get_node(id) else {
        return (None, id);
    };
    let owner_id = node.owner().unwrap_or(id);
    let owner = tree.get_node(owner_id).unwrap_or(node);
    match owner.get_property("_instance_path") {
        Variant::String(s) if !s.is_empty() => {
            (Some(s), NodeId::from_object_id(ObjectId::from_raw(0)))
        }
        _ => (None, owner_id),
    }
}

/// Godot's `resource_path` for an embedded sub-resource: `"<scene>::<id>"`, or
/// empty when the owning scene's file is not recorded.
fn resource_path_for(tree: &SceneTree, id: NodeId, sub_id: &str) -> String {
    match scope_of(tree, id).0 {
        Some(scene) => format!("{scene}::{sub_id}"),
        None => String::new(),
    }
}

/// ClassDB's registered default for `class.field`, if any.
fn class_default(class: &str, field: &str) -> Option<Variant> {
    gdobject::class_db::get_property_list(class)
        .into_iter()
        .find(|p| p.name == field)
        .map(|p| p.default_value)
}

/// One stored field, converted to what a script sees: a nested sub-resource
/// becomes its own bound snapshot, an `ExtResource` the loaded resource.
fn script_value(
    tree: &SceneTree,
    id: NodeId,
    node: &Node,
    path: &str,
    field: &str,
    raw: Variant,
    depth: usize,
) -> Variant {
    if let Variant::String(text) = &raw {
        if text.starts_with("SubResource") {
            let nested = format!("{path}:{field}");
            if depth < MAX_DEPTH {
                if let Some(r) = snapshot_at(tree, id, &nested, depth + 1) {
                    return Variant::Resource(Box::new(r));
                }
            }
        } else if text.starts_with("ExtResource") {
            let key = format!("__extres__{}{field}", body_prefix(path));
            if let Variant::String(res_path) = node.get_property(&key) {
                if !res_path.is_empty() {
                    return gdscript_interop::interpreter::load_resource_value(&res_path);
                }
            }
        }
    }
    raw
}

/// The stored value of one field, in the order its consumers see it: the
/// specialised mirror (which the editor edits in place) for a plain value, the
/// verbatim body for a reference, else nothing.
fn stored_field(node: &Node, path: &str, field: &str) -> Option<Variant> {
    let verbatim = node.get_property(&format!("{}{field}", body_prefix(path)));
    let is_ref = matches!(&verbatim, Variant::String(s)
        if s.starts_with("SubResource") || s.starts_with("ExtResource"));
    if !is_ref {
        if mirrors_shader_parameters(path, field) {
            let v = node.get_property(field);
            if !v.is_nil() {
                return Some(v);
            }
        }
        if let Some(prefix) = mirror_prefix(node, path) {
            let v = node.get_property(&format!("{prefix}{field}"));
            if !v.is_nil() {
                return Some(v);
            }
        }
    }
    (!verbatim.is_nil()).then_some(verbatim)
}

fn snapshot_at(tree: &SceneTree, id: NodeId, path: &str, depth: usize) -> Option<ResourceRef> {
    let node = tree.get_node(id)?;
    let meta = meta_of(node, path)?;
    let (class, sub_id) = meta.rsplit_once("::")?;
    let mut properties: std::collections::HashMap<String, Variant> =
        gdobject::class_db::get_property_list(class)
            .into_iter()
            .map(|p| (p.name, p.default_value))
            .collect();
    let prefix = body_prefix(path);
    let fields: Vec<String> = node
        .properties()
        .filter_map(|(k, _)| {
            let f = k.strip_prefix(prefix.as_str())?;
            (!f.starts_with("::")).then(|| f.to_string())
        })
        .collect();
    for field in fields {
        if let Some(raw) = stored_field(node, path, &field) {
            let v = script_value(tree, id, node, path, &field, raw, depth);
            properties.insert(field, v);
        }
    }
    let binding = binding_for(tree, id, path)?;
    properties.insert(BOUND_RESOURCE_KEY.to_string(), Variant::String(binding));
    let resource_path = resource_path_for(tree, id, sub_id);
    properties.insert(
        "resource_path".to_string(),
        Variant::String(resource_path.clone()),
    );
    Some(ResourceRef {
        path: resource_path,
        class_name: class.to_string(),
        properties,
    })
}

/// What a script reads for `node.<prop>` when that property is an inline
/// sub-resource: a bound snapshot, or `None` when it is anything else.
pub fn script_resource_value(tree: &SceneTree, id: NodeId, prop: &str) -> Option<Variant> {
    let node = tree.get_node(id)?;
    match node.get_property(prop) {
        Variant::String(s) if s.starts_with("SubResource") => {}
        _ => return None,
    }
    snapshot_at(tree, id, prop, 0).map(|r| Variant::Resource(Box::new(r)))
}

/// A fresh snapshot of the sub-resource `binding` names.
pub fn bound_snapshot(tree: &SceneTree, binding: &str) -> Option<ResourceRef> {
    let (id, path) = resolve(tree, binding)?;
    snapshot_at(tree, id, &path, 0)
}

/// One field of the sub-resource `binding` names, read live.
pub fn bound_get(tree: &SceneTree, binding: &str, field: &str) -> Option<Variant> {
    let (id, path) = resolve(tree, binding)?;
    let path = path.as_str();
    let node = tree.get_node(id)?;
    let meta = meta_of(node, path)?;
    let (class, sub_id) = meta.rsplit_once("::")?;
    if field == "resource_path" {
        return Some(Variant::String(resource_path_for(tree, id, sub_id)));
    }
    if let Some(raw) = stored_field(node, path, field) {
        return Some(script_value(tree, id, node, path, field, raw, 0));
    }
    // A nested sub-resource the body names but that was not flattened (deeper
    // than the flattening goes) still answers as its default, never as Nil.
    class_default(class, field)
}

/// Every `(node, property path)` holding the sub-resource `meta` within
/// `scope` (a [`scope_key`]).
fn holders(tree: &SceneTree, meta: &str, scope: &str) -> Vec<(NodeId, String)> {
    let mut out = Vec::new();
    // A scope keyed on its instance root is that root's subtree — which also
    // reaches a scene instanced but not yet added to the tree (truck_town's
    // `town.setup()` writes the environment before `add_child(town)`).
    let candidates = match scope
        .strip_prefix("node:")
        .and_then(|raw| raw.parse::<u64>().ok())
    {
        Some(raw) => {
            let mut ids = Vec::new();
            tree.collect_subtree_top_down(
                NodeId::from_object_id(ObjectId::from_raw(raw)),
                &mut ids,
            );
            ids
        }
        None => tree.all_nodes_in_tree_order(),
    };
    for id in candidates {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        let paths: Vec<String> = node
            .properties()
            .filter(|(_, v)| matches!(v, Variant::String(s) if s == meta))
            .filter_map(|(k, _)| {
                if let Some(p) = k.strip_prefix("__resref__") {
                    return Some(p.to_string());
                }
                k.strip_prefix("__subres__")?
                    .strip_suffix("__::resref")
                    .map(str::to_string)
            })
            .collect();
        if paths.is_empty() || scope_key(tree, id) != scope {
            continue;
        }
        out.extend(paths.into_iter().map(|p| (id, p)));
    }
    out
}

/// Writes one field onto one holder's companions.
fn write_field(node: &mut Node, path: &str, field: &str, value: &Variant) {
    let verbatim = format!("{}{field}", body_prefix(path));
    let extres = format!("__extres__{verbatim}");
    // A resource assigned by path (`sky_material = preload("res://x.tres")`)
    // replaces an `ExtResource` reference: keep the resolved-path companion in
    // step so a consumer reading it does not keep the old file.
    match value {
        Variant::Resource(r) if !r.path.is_empty() && !r.path.contains("::") => {
            node.set_property(&extres, Variant::String(r.path.clone()));
            node.remove_property(&format!("__extuid__{verbatim}"));
        }
        _ => {
            if node.has_property(&extres) {
                node.remove_property(&extres);
                node.remove_property(&format!("__extuid__{verbatim}"));
            }
        }
    }
    node.set_property(&verbatim, value.clone());
    if mirrors_shader_parameters(path, field) {
        node.set_property(field, value.clone());
    }
    if let Some(prefix) = mirror_prefix(node, path) {
        node.set_property(&format!("{prefix}{field}"), value.clone());
    }
}

/// Writes one field of the sub-resource `binding` names onto every holder.
pub fn bound_set(tree: &mut SceneTree, binding: &str, field: &str, value: Variant) -> bool {
    let Some((id, path)) = resolve(tree, binding) else {
        return false;
    };
    let Some((meta, scope)) = binding.split_once('|') else {
        return false;
    };
    if field == "resource_path" {
        // Read-only for an embedded resource in practice; accept and ignore.
        return true;
    }
    let mut targets = holders(tree, meta, scope);
    if !targets.iter().any(|(h, p)| *h == id && *p == path) {
        // The resolved holder, even when the scan cannot see it (a node not
        // yet in tree order).
        targets.push((id, path));
    }
    for (holder, hpath) in targets {
        if let Some(node) = tree.get_node_mut(holder) {
            write_field(node, &hpath, field, &value);
        }
    }
    true
}

/// `target.<prop> = <bound resource>`: makes `target` another holder of the
/// sub-resource `binding` names, by copying the reference text, its resref and
/// every companion of it from the source node (pat-nyn3w).
///
/// Returns `false` (the caller then stores the value as it is) when the
/// binding does not resolve or `prop` is not a plain top-level property.
pub fn adopt(tree: &mut SceneTree, target: NodeId, prop: &str, binding: &str) -> bool {
    let Some((source, spath)) = resolve(tree, binding) else {
        return false;
    };
    if prop.contains(':') {
        return false;
    }
    let Some(src) = tree.get_node(source) else {
        return false;
    };
    let Some(meta) = meta_of(src, &spath) else {
        return false;
    };
    let Some((_, sub_id)) = meta.rsplit_once("::") else {
        return false;
    };
    let sub_text = format!("SubResource:{sub_id}");
    // The source's body (verbatim + nested) and resolved-path companions, under
    // the target's property name.
    let body = format!("__subres__{spath}__");
    let nested = format!("__subres__{spath}:");
    let mut copied: Vec<(String, Variant)> = Vec::new();
    for (k, v) in src.properties() {
        let (is_extres, rest) = match k.strip_prefix("__extres__") {
            Some(r) => (true, r),
            None => (false, k.as_str()),
        };
        let renamed = if let Some(tail) = rest.strip_prefix(body.as_str()) {
            if tail.starts_with("::") {
                continue;
            }
            format!("__subres__{prop}__{tail}")
        } else if let Some(tail) = rest.strip_prefix(nested.as_str()) {
            format!("__subres__{prop}:{tail}")
        } else {
            continue;
        };
        let key = if is_extres {
            format!("__extres__{renamed}")
        } else {
            renamed
        };
        copied.push((key, v.clone()));
    }
    // The specialised mirror, when both sides have one.
    let mirror = mirror_prefix(src, &spath).zip(crate::scene_saver::companion_prefix_for(prop));
    if let Some((from, to)) = &mirror {
        for (k, v) in src.properties() {
            if let Some(tail) = k.strip_prefix(from.as_str()) {
                copied.push((format!("{to}{tail}"), v.clone()));
            }
        }
    }
    let Some(node) = tree.get_node_mut(target) else {
        return false;
    };
    // Drop the target's previous sub-resource under this property first, so no
    // stale field of the old resource survives alongside the new one.
    let old_body = format!("__subres__{prop}__");
    let old_nested = format!("__subres__{prop}:");
    let old_mirror = mirror.as_ref().map(|(_, to)| to.clone());
    let stale: Vec<String> = node
        .properties()
        .map(|(k, _)| k.clone())
        .filter(|k| {
            let rest = k.strip_prefix("__extres__").unwrap_or(k);
            rest.starts_with(old_body.as_str())
                || rest.starts_with(old_nested.as_str())
                || old_mirror.as_deref().is_some_and(|m| k.starts_with(m))
        })
        .collect();
    for k in stale {
        node.remove_property(&k);
    }
    for (k, v) in copied {
        node.set_property(&k, v);
    }
    node.set_property(&format!("__resref__{prop}"), Variant::String(meta));
    node.set_property(prop, Variant::String(sub_text));
    true
}
