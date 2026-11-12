//! pat-s3rrv.256 — the graph-organization nodes: `VisualShaderNodeFrame`,
//! `VisualShaderNodeComment` and `VisualShaderNodeReroute`.
//!
//! # What these nodes are
//!
//! Godot's visual-shader editor has three classes that exist purely to make a
//! graph readable and contribute **nothing** to the generated shader:
//!
//! * `VisualShaderNodeFrame` — a resizable box drawn behind a group of nodes,
//!   carrying a `title` and the `attached_nodes` it visually contains.
//! * `VisualShaderNodeComment` — deprecated in 4.6 ("This class has no function
//!   anymore and only exists for compatibility"), kept so old `.tres` files
//!   still load. It inherits `Frame`.
//! * `VisualShaderNodeReroute` — a pass-through routing dot. `A -> R -> B` must
//!   compile **exactly** as `A -> B`; the reroute is a wire elbow, not an
//!   operation.
//!
//! # Why this is a pre-pass over the `Resource`, not a fix in the loader
//!
//! `visual_shader_load::visual_shader_from_resource` maps each node class to a
//! `VisualShaderNodeType` and **drops** what it cannot map, recording a
//! diagnostic. That is right for a genuinely unsupported node and wrong for
//! these three:
//!
//! * a Frame reported as "dropped from the graph" is a false alarm — it was
//!   never going to emit code, and saying so buries the real diagnostics that
//!   sit in the same list;
//! * a dropped **Reroute** silently breaks the graph. Its id vanishes while the
//!   connection quads still name it, so `A -> R` and `R -> B` both dangle and
//!   the edge `A -> B` the author drew is simply gone. The shader still
//!   compiles, which is what makes it dangerous.
//!
//! Once the loader has dropped the node the reroute's id is gone, so this
//! **cannot** be repaired by a pass over the finished `VisualShader` — the
//! information needed to reconnect the edge no longer exists. Hence a pre-pass:
//! rewrite the `Resource` so the graph handed to the loader is already the
//! reroute-free, annotation-free equivalent.
//!
//! ⚠️ The rewrite is applied to a **copy** used for codegen only. The caller's
//! `Resource` keeps its Frames and Reroutes, so a `.tres` still round-trips
//! through `TresLoader`/`TresSaver` with every annotation intact — which is
//! exactly what the editor needs when it saves the file back.

use std::collections::{HashMap, HashSet};

use gdresource::resource::Resource;
use gdvariant::Variant;

/// Classes that are pure annotation: they carry no shader code by design.
///
/// `Comment` inherits `Frame` in Godot 4.6 and is deprecated; both are listed
/// explicitly because the `.tres` records a concrete class name and there is no
/// inheritance lookup at this layer.
pub const ANNOTATION_CLASSES: &[&str] = &["VisualShaderNodeFrame", "VisualShaderNodeComment"];

/// The pass-through routing node, collapsed into a direct edge.
pub const REROUTE_CLASS: &str = "VisualShaderNodeReroute";

/// A reroute chain longer than this is treated as a cycle and cut.
///
/// A `.tres` can name a reroute whose input is itself — hand-edited files and
/// fuzzed input both reach it — and the resolver below walks the chain, so an
/// unbounded walk would hang the loader rather than fail it.
const MAX_REROUTE_CHAIN: usize = 64;

/// `(function, id)` parsed out of a `nodes/<func>/<id>/<field>` property key.
fn split_node_key(key: &str) -> Option<(&str, u32, &str)> {
    let rest = key.strip_prefix("nodes/")?;
    let (head, field) = rest.rsplit_once('/')?;
    let (func, id_str) = head.rsplit_once('/')?;
    Some((func, id_str.parse().ok()?, field))
}

/// Maps every node id in `res` to the class name of its sub-resource.
fn node_classes(res: &Resource) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    for (key, value) in res.properties() {
        let Some((_func, id, field)) = split_node_key(key) else {
            continue;
        };
        if field != "node" {
            continue;
        }
        let Some(sub_id) = sub_resource_id(value) else {
            continue;
        };
        if let Some(sub) = res.subresources.get(&sub_id) {
            out.insert(id, sub.class_name.clone());
        }
    }
    out
}

/// Extracts the id from a `SubResource("X")` reference.
///
/// ⚠️ The `.tres` parser does **not** keep the literal call text: it rewrites
/// `SubResource("Color_1")` to the string `"SubResource:Color_1"`. Matching only
/// the call syntax finds no nodes at all, so this accepts both forms.
///
/// Deliberately a local copy of `visual_shader_load`'s private helper of the
/// same name rather than a `pub` on that function — this pass is a pre-pass
/// over the same key format, and widening another module's API to share ten
/// lines is not worth the coupling.
fn sub_resource_id(value: &Variant) -> Option<String> {
    let s = match value {
        Variant::String(s) => s.as_str(),
        _ => return None,
    };
    if let Some(id) = s.strip_prefix("SubResource:") {
        return Some(id.to_string());
    }
    let inner = s.strip_prefix("SubResource(")?.strip_suffix(')')?;
    Some(inner.trim_matches('"').to_string())
}

/// The graph-organization nodes in `res`, split by how they are removed.
struct Organization {
    /// Ids of `Frame`/`Comment` nodes.
    annotations: HashSet<u32>,
    /// Ids of `Reroute` nodes.
    reroutes: HashSet<u32>,
}

impl Organization {
    fn of(res: &Resource) -> Self {
        let mut annotations = HashSet::new();
        let mut reroutes = HashSet::new();
        for (id, class) in node_classes(res) {
            if class == REROUTE_CLASS {
                reroutes.insert(id);
            } else if ANNOTATION_CLASSES.contains(&class.as_str()) {
                annotations.insert(id);
            }
        }
        Self {
            annotations,
            reroutes,
        }
    }

    fn is_empty(&self) -> bool {
        self.annotations.is_empty() && self.reroutes.is_empty()
    }

    fn contains(&self, id: u32) -> bool {
        self.annotations.contains(&id) || self.reroutes.contains(&id)
    }
}

/// Rewrites `res` into the graph a reroute-free, annotation-free author would
/// have drawn by hand.
///
/// Returns `None` when there is nothing to do, so the overwhelmingly common
/// graph — one with no organization nodes at all — pays no clone.
pub fn collapse_graph_organization(res: &Resource) -> Option<Resource> {
    if res.class_name != "VisualShader" {
        return None;
    }
    let org = Organization::of(res);
    if org.is_empty() {
        return None;
    }

    let mut out = res.clone();

    // -- drop the organization nodes' own rows -------------------------------
    // Keys are collected first: `properties()` borrows the resource and the
    // removal below mutates it.
    let doomed: Vec<String> = out
        .properties()
        .filter_map(|(key, _)| {
            let (_func, id, _field) = split_node_key(key)?;
            org.contains(id).then(|| key.clone())
        })
        .collect();
    for key in doomed {
        out.remove_property(&key);
    }

    // -- rewrite the connection quads ---------------------------------------
    let conn_keys: Vec<String> = out
        .properties()
        .filter(|(key, _)| key.starts_with("nodes/") && key.ends_with("/connections"))
        .map(|(key, _)| key.clone())
        .collect();

    for key in conn_keys {
        let Some(Variant::PackedInt32Array(quads)) = out.get_property(&key) else {
            continue;
        };
        let edges: Vec<[i32; 4]> = quads
            .chunks_exact(4)
            .map(|q| [q[0], q[1], q[2], q[3]])
            .collect();

        // Where each reroute gets its value from, before any resolution: a
        // reroute has exactly one input port, so at most one edge feeds it.
        let mut feeds: HashMap<u32, (i32, i32)> = HashMap::new();
        for e in &edges {
            let to_node = e[2].max(0) as u32;
            if org.reroutes.contains(&to_node) {
                feeds.insert(to_node, (e[0], e[1]));
            }
        }

        let mut rewritten: Vec<i32> = Vec::with_capacity(edges.len() * 4);
        for e in &edges {
            let (from_node, from_port, to_node, to_port) = (e[0], e[1], e[2], e[3]);
            let to_u = to_node.max(0) as u32;
            // An edge INTO an organization node dies with it; the edge out of a
            // reroute is the one that survives, carrying the original source.
            if org.contains(to_u) {
                continue;
            }
            let from_u = from_node.max(0) as u32;
            if org.annotations.contains(&from_u) {
                // An annotation node cannot carry a value. Godot never writes
                // such an edge, so this is defensive against a hand-edited file.
                continue;
            }
            let Some((src_node, src_port)) = resolve_source(from_node, from_port, &org, &feeds)
            else {
                // A reroute with nothing feeding it — a dangling wire elbow.
                // Dropping the edge matches what the author sees: no value
                // arrives at the destination.
                continue;
            };
            rewritten.extend_from_slice(&[src_node, src_port, to_node, to_port]);
        }

        if rewritten.is_empty() {
            out.remove_property(&key);
        } else {
            out.set_property(key, Variant::PackedInt32Array(rewritten));
        }
    }

    Some(out)
}

/// Walks back through any chain of reroutes to the real source of an edge.
///
/// `A -> R1 -> R2 -> B` must compile as `A -> B`, so the walk is transitive
/// rather than a single hop.
fn resolve_source(
    from_node: i32,
    from_port: i32,
    org: &Organization,
    feeds: &HashMap<u32, (i32, i32)>,
) -> Option<(i32, i32)> {
    let (mut node, mut port) = (from_node, from_port);
    for _ in 0..MAX_REROUTE_CHAIN {
        let id = node.max(0) as u32;
        if !org.reroutes.contains(&id) {
            return Some((node, port));
        }
        let (up_node, up_port) = *feeds.get(&id)?;
        node = up_node;
        port = up_port;
    }
    // A cycle. Cutting the edge keeps the loader terminating; the graph the
    // author drew has no meaning here anyway.
    None
}
