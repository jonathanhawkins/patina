//! Build a live node subtree out of an imported glTF document (pat-mgixk.23).
//!
//! Godot imports a `.glb`/`.gltf` through `ResourceImporterScene` into a
//! **PackedScene**, so `load("res://m.glb").instantiate()` yields a `Node3D`
//! hierarchy: per-node transforms, `MeshInstance3D` nodes whose mesh carries one
//! surface per glTF primitive, `Camera3D`/light nodes, and an `AnimationPlayer`
//! holding the clips (`modules/gltf/gltf_document.cpp` `_generate_scene_node`).
//!
//! Patina parsed all of that and then had nowhere to put it. `import_gltf_asset`
//! returned a resource of class `GltfDocument`, so `instantiate()` refused it;
//! the only runtime consumer of glTF geometry was the render server noticing a
//! `MeshInstance3D.mesh` string ending in `.glb` — which is not a Godot
//! construct — and taking the FIRST primitive of the whole file for every node.
//! With no transforms applied either, every node drew the same mesh at the
//! origin, and a node with no match fell back to `Mesh3D::cube(1.0)`.
//!
//! This module is the missing assembly step. Everything it needs already
//! existed: the importer records node hierarchy and TRS (`gltf_import.rs`
//! `node_trs`, including the `matrix` form), one subresource per primitive keyed
//! `mesh_<m>_prim_<p>` and tagged with `mesh_index`, and
//! `animations_from_gltf_resource` builds the clips. `Mesh3D` has had
//! multi-surface support with paired per-surface materials all along
//! (`gdrender3d` reads `Mesh3D::surfaces`).
//!
//! ## Mesh addressing
//!
//! A built `MeshInstance3D` gets BOTH `mesh` (the document path, which is what
//! `node3d::get_mesh_path` reads) and `gltf_mesh_index`. The index is the part
//! that matters: without it the render server cannot tell which of the file's
//! meshes a node wants, which is why one mesh leaked onto every node. That
//! property name is not new — `GLTFDocument.generate_scene` already set it and
//! nothing ever read it.

use gdcore::math::Vector3;
use gdcore::math3d::Quaternion;
use gdresource::Resource;
use gdvariant::Variant;

use crate::node::{Node, NodeId};
use crate::scene_tree::SceneTree;

/// Reads a fixed-length float array property off a glTF node dictionary.
///
/// Returns `None` rather than a partial vector when the length is wrong, so a
/// malformed node falls back to its identity default instead of silently
/// reading a zero for the missing component.
fn floats(dict: &gdvariant::SharedDictionary, key: &str, want: usize) -> Option<Vec<f32>> {
    match dict.get(key) {
        Some(Variant::Array(a)) => {
            let items = a.to_vec();
            if items.len() != want {
                return None;
            }
            let mut out = Vec::with_capacity(want);
            for item in items {
                match item {
                    Variant::Float(f) => out.push(f as f32),
                    Variant::Int(i) => out.push(i as f32),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn int_of(dict: &gdvariant::SharedDictionary, key: &str) -> Option<i64> {
    match dict.get(key) {
        Some(Variant::Int(i)) => Some(i),
        Some(Variant::Float(f)) => Some(f as i64),
        _ => None,
    }
}

/// The node class Godot gives a glTF node, from which of `mesh`/`camera`/`light`
/// it carries (`gltf_document.cpp` `_generate_scene_node`).
///
/// A light's concrete class comes from its KHR_lights_punctual `type`, which the
/// importer stored on the document's `lights` array — so the light INDEX has to
/// be resolved against that, not guessed from the node.
fn node_class(dict: &gdvariant::SharedDictionary, doc: &Resource) -> &'static str {
    if int_of(dict, "mesh").is_some() {
        return "MeshInstance3D";
    }
    if int_of(dict, "camera").is_some() {
        return "Camera3D";
    }
    if let Some(light_index) = int_of(dict, "light") {
        let kind = match doc.get_property("lights") {
            Some(Variant::Array(lights)) => lights
                .to_vec()
                .get(light_index.max(0) as usize)
                .and_then(|l| match l {
                    Variant::Dictionary(d) => match d.get("type") {
                        Some(Variant::String(s)) => Some(s),
                        _ => None,
                    },
                    _ => None,
                }),
            _ => None,
        };
        return match kind.as_deref() {
            Some("directional") => "DirectionalLight3D",
            Some("spot") => "SpotLight3D",
            // glTF's "point" is Godot's OmniLight3D. An unknown or absent type
            // also lands here: a light that exists but cannot be classified is
            // better as an omni than dropped from the scene.
            _ => "OmniLight3D",
        };
    }
    "Node3D"
}

/// Applies a glTF node's TRS to a freshly created Node3D-derived node.
///
/// ⚠️ glTF rotation is a QUATERNION and Patina stores `rotation` as Godot's
/// Euler `Vector3`, so it is converted rather than copied componentwise — four
/// floats written into a three-component property is how a rig ends up subtly
/// mis-rotated. `Quaternion::to_euler` uses the YXZ convention Godot does.
fn apply_transform(tree: &mut SceneTree, node_id: NodeId, dict: &gdvariant::SharedDictionary) {
    let t = floats(dict, "translation", 3).unwrap_or_else(|| vec![0.0, 0.0, 0.0]);
    let r = floats(dict, "rotation", 4).unwrap_or_else(|| vec![0.0, 0.0, 0.0, 1.0]);
    let s = floats(dict, "scale", 3).unwrap_or_else(|| vec![1.0, 1.0, 1.0]);

    crate::node3d::set_position(tree, node_id, Vector3::new(t[0], t[1], t[2]));
    let quat = Quaternion {
        x: r[0],
        y: r[1],
        z: r[2],
        w: r[3],
    };
    crate::node3d::set_rotation(tree, node_id, quat.to_euler());
    crate::node3d::set_scale(tree, node_id, Vector3::new(s[0], s[1], s[2]));
}

/// Creates the node for glTF node `index` under `parent`, then its children.
///
/// Depth-first in document order, which is the order Godot's importer walks, so
/// sibling order in the built tree matches the file.
fn build_node(
    tree: &mut SceneTree,
    parent: NodeId,
    doc: &Resource,
    nodes: &[Variant],
    index: usize,
    depth: usize,
) -> Option<NodeId> {
    // A glTF `children` array may contain a cycle, and nothing upstream of here
    // validates that. Without a bound, a hand-made or corrupt file would recurse
    // until the stack dies — a hang rather than an import error.
    const MAX_DEPTH: usize = 256;
    if depth > MAX_DEPTH {
        return None;
    }
    let dict = match nodes.get(index) {
        Some(Variant::Dictionary(d)) => d.clone(),
        _ => return None,
    };
    let name = match dict.get("name") {
        Some(Variant::String(s)) if !s.is_empty() => s,
        // Godot falls back to a positional name so every node is addressable by
        // path even when the file left it unnamed.
        _ => format!("Node{index}"),
    };
    let class = node_class(&dict, doc);
    let node_id = tree.add_child(parent, Node::new(&name, class)).ok()?;
    apply_transform(tree, node_id, &dict);

    if let Some(mesh_index) = int_of(&dict, "mesh") {
        if let Some(node) = tree.get_node_mut(node_id) {
            // `mesh` is what `node3d::get_mesh_path` reads; `gltf_mesh_index`
            // is what tells the render server WHICH mesh of the file this node
            // uses. Both are required — see the module header.
            node.set_property("mesh", Variant::String(doc.path.clone()));
            node.set_property("gltf_mesh_index", Variant::Int(mesh_index));
        }
    }

    if let Some(Variant::Array(kids)) = dict.get("children") {
        for kid in kids.to_vec() {
            if let Variant::Int(kid_index) = kid {
                if kid_index >= 0 {
                    build_node(tree, node_id, doc, nodes, kid_index as usize, depth + 1);
                }
            }
        }
    }
    Some(node_id)
}

/// Instantiates `doc`'s default scene as a child of `parent`, returning the root
/// node that was created, or `None` when the document declares no nodes.
///
/// The returned root is always a single node, named after the glTF scene (or the
/// file stem), with the file's root nodes beneath it. Godot promotes a lone root
/// to BE the scene root, but a document with several roots still needs a wrapper
/// — always wrapping keeps `instantiate()` returning one node, which is what
/// both `add_child(scene.instantiate())` and a `.tscn` `instance=ExtResource`
/// require.
pub fn build_gltf_subtree(tree: &mut SceneTree, parent: NodeId, doc: &Resource) -> Option<NodeId> {
    let nodes = match doc.get_property("nodes") {
        Some(Variant::Array(a)) => a.to_vec(),
        _ => return None,
    };
    if nodes.is_empty() {
        return None;
    }

    let scene_name = match doc.get_property("scene_name") {
        Some(Variant::String(s)) if !s.is_empty() => s.clone(),
        _ => std::path::Path::new(&doc.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("GltfScene")
            .to_string(),
    };
    let root = tree
        .add_child(parent, Node::new(&scene_name, "Node3D"))
        .ok()?;

    // `scene_roots` is the document's default-scene root list. Falling back to
    // "every node that is nobody's child" matters: a glTF file is allowed to
    // omit `scenes`, and treating node 0 as the only root would silently drop
    // the rest of the file.
    let roots: Vec<usize> = match doc.get_property("scene_roots") {
        Some(Variant::Array(a)) => a
            .to_vec()
            .into_iter()
            .filter_map(|v| match v {
                Variant::Int(i) if i >= 0 => Some(i as usize),
                _ => None,
            })
            .collect(),
        _ => {
            let mut is_child = vec![false; nodes.len()];
            for entry in &nodes {
                if let Variant::Dictionary(d) = entry {
                    if let Some(Variant::Array(kids)) = d.get("children") {
                        for kid in kids.to_vec() {
                            if let Variant::Int(i) = kid {
                                if i >= 0 && (i as usize) < is_child.len() {
                                    is_child[i as usize] = true;
                                }
                            }
                        }
                    }
                }
            }
            (0..nodes.len()).filter(|i| !is_child[*i]).collect()
        }
    };

    for index in roots {
        build_node(tree, root, doc, &nodes, index, 0);
    }

    // The clips live on an AnimationPlayer under the scene root, as Godot's
    // importer places them. `animations_from_gltf_resource` already built them
    // and had no non-test caller at all before this.
    let animations = crate::animation::animations_from_gltf_resource(doc);
    if !animations.is_empty() {
        if let Ok(player_node) =
            tree.add_child(root, Node::new("AnimationPlayer", "AnimationPlayer"))
        {
            let mut player = crate::animation::AnimationPlayer::new();
            for clip in animations {
                player.add_animation(clip);
            }
            let _ = tree.attach_animation_player(player_node, player);
        }
    }

    Some(root)
}
