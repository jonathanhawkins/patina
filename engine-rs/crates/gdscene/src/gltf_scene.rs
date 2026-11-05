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

use std::collections::HashMap;

use gdcore::math::Vector3;
use gdcore::math3d::{Basis, Quaternion, Transform3D};
use gdresource::Resource;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::packed_scene::{PackedScene, SyntheticNode};
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


/// Instantiates `doc`'s default scene as a child of `parent`, returning the root
/// node that was created, or `None` when the document declares no nodes.
///
/// The returned root is always a single node, named after the glTF scene (or the
/// file stem), with the file's root nodes beneath it. Godot promotes a lone root
/// to BE the scene root, but a document with several roots still needs a wrapper
/// — always wrapping keeps `instantiate()` returning one node, which is what
/// both `add_child(scene.instantiate())` and a `.tscn` `instance=ExtResource`
/// require.
///
/// 🔑 Built from [`packed_scene_from_gltf`], the SAME scene a `.tscn`'s
/// `instance=ExtResource("m.glb")` instances, so a script's
/// `load("m.glb").instantiate()` and a scene's instance of it cannot disagree
/// about the hierarchy (pat-m2jgd). This used to build its own tree, with every
/// joint as a plain `Node3D` and no `Skeleton3D` at all.
pub fn build_gltf_subtree(tree: &mut SceneTree, parent: NodeId, doc: &Resource) -> Option<NodeId> {
    let scene = packed_scene_from_gltf(doc)?;
    crate::packed_scene::add_packed_scene_to_tree_with_subscenes(tree, parent, &scene, &|_| None)
        .map_err(|e| tracing::error!("glTF scene '{}' failed to instance: {e}", doc.path))
        .ok()
}

/// Builds the imported scene of a glTF document as a [`PackedScene`], the way
/// Godot's `ResourceImporterScene` does for a `.glb`, so it instances through
/// the same path as a `.tscn` — including as a `.tscn`'s
/// `instance=ExtResource("player.glb")` with editable children (pat-m2jgd).
///
/// The hierarchy follows `gltf_document.cpp` `_generate_scene_node_tree`:
///
/// - a `Node3D` root (named after the glTF scene, else the file stem) holding
///   the document's root nodes;
/// - a skin's JOINTS are not nodes: they become the bones of a generated
///   `Skeleton3D` (named `Skeleton3D`, uniquified against the document's node
///   names), placed where the skeleton's root joint sits — so the official 3D
///   platformer's `player.glb` yields `Skeleton/Skeleton3D`, which its
///   `player.tscn` edits as `parent="Player/Skeleton/Skeleton3D"`;
/// - a non-joint child of a joint hangs off a `BoneAttachment3D` named after
///   that bone;
/// - a skinned mesh is reparented under its skeleton with an identity
///   transform (`_process_mesh_instances`), carrying the skin's binds;
/// - an `AnimationPlayer` holds the clips, with joint channels keyed
///   `<skeleton path>:<bone>` like Godot's `_import_animation`.
///
/// Returns `None` when the document has no nodes.
pub fn packed_scene_from_gltf(doc: &Resource) -> Option<PackedScene> {
    let nodes: Vec<gdvariant::SharedDictionary> = match doc.get_property("nodes") {
        Some(Variant::Array(a)) => a
            .to_vec()
            .into_iter()
            .map(|v| match v {
                Variant::Dictionary(d) => d,
                _ => gdvariant::SharedDictionary::default(),
            })
            .collect(),
        _ => return None,
    };
    let n = nodes.len();
    if n == 0 {
        return None;
    }

    // -- graph --------------------------------------------------------------
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut parent: Vec<Option<usize>> = vec![None; n];
    for (i, dict) in nodes.iter().enumerate() {
        if let Some(Variant::Array(kids)) = dict.get("children") {
            for kid in kids.to_vec() {
                if let Variant::Int(k) = kid {
                    let k = k as usize;
                    // A node may have ONE parent; a cycle or a second parent
                    // is a malformed file, and keeping the first edge keeps
                    // the walk below finite.
                    if k < n && k != i && parent[k].is_none() {
                        parent[k] = Some(i);
                        children[i].push(k);
                    }
                }
            }
        }
    }

    // -- skeletons (`_determine_skeletons`) ---------------------------------
    let skins = read_skins(doc, n);
    let mut skel_of: Vec<Option<usize>> = vec![None; n];
    let mut skin_to_skel: Vec<Option<usize>> = vec![None; skins.len()];
    let mut skel_count = 0usize;
    for (si, skin) in skins.iter().enumerate() {
        if skin.joints.is_empty() {
            continue;
        }
        let mut set: std::collections::HashSet<usize> = skin.joints.iter().copied().collect();
        // Nodes BETWEEN two joints are bones too (`_expand_skin`), or the
        // bone chain would have a hole.
        for &j in &skin.joints {
            let mut walked = Vec::new();
            let mut cursor = parent[j];
            while let Some(p) = cursor {
                if set.contains(&p) {
                    set.extend(walked.drain(..));
                    break;
                }
                walked.push(p);
                cursor = parent[p];
            }
        }
        // Skins sharing a joint share ONE skeleton.
        let existing: Vec<usize> = {
            let mut v: Vec<usize> = set.iter().filter_map(|&j| skel_of[j]).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let id = match existing.first() {
            Some(&first) => {
                for other in &existing[1..] {
                    for s in skel_of.iter_mut().filter(|s| **s == Some(*other)) {
                        *s = Some(first);
                    }
                    for s in skin_to_skel.iter_mut().filter(|s| **s == Some(*other)) {
                        *s = Some(first);
                    }
                }
                first
            }
            None => {
                skel_count += 1;
                skel_count - 1
            }
        };
        for &j in &set {
            skel_of[j] = Some(id);
        }
        skin_to_skel[si] = Some(id);
    }

    // -- names (`_assign_node_names`, then `_create_skeletons`) -------------
    // Godot keeps ONE unique-name set for the whole document; joints are
    // skipped (their names are bone names, unique per skeleton instead).
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut node_names: Vec<String> = vec![String::new(); n];
    for (i, dict) in nodes.iter().enumerate() {
        if skel_of[i].is_some() {
            continue;
        }
        let raw = match dict.get("name") {
            Some(Variant::String(s)) if !s.is_empty() => s,
            _ if int_of(dict, "mesh").is_some() => "Mesh".to_string(),
            _ if int_of(dict, "camera").is_some() => "Camera".to_string(),
            _ => "Node".to_string(),
        };
        node_names[i] = gen_unique_name(&mut used, &validate_node_name(&raw));
    }
    let skel_ids: Vec<usize> = {
        let mut v: Vec<usize> = skel_of.iter().flatten().copied().collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let mut skel_names: HashMap<usize, String> = HashMap::new();
    for &k in &skel_ids {
        skel_names.insert(k, gen_unique_name(&mut used, "Skeleton3D"));
    }

    // -- bones: depth-first from each sorted root, children in file order ---
    // (bone order matters: a `.tscn` override addresses `bones/<idx>/…`; the
    // platformer's enemy.tscn puts `eyes` at bone 2, right after its parent
    // `body`, which is depth-first, not breadth-first).
    let mut bone_of: Vec<Option<usize>> = vec![None; n];
    let mut bone_names: Vec<String> = vec![String::new(); n];
    let mut skel_props: HashMap<usize, HashMap<String, Variant>> = HashMap::new();
    for &k in &skel_ids {
        let mut roots: Vec<usize> = (0..n)
            .filter(|&i| skel_of[i] == Some(k) && parent[i].map_or(true, |p| skel_of[p] != Some(k)))
            .collect();
        roots.sort_unstable();
        let mut props = HashMap::new();
        let mut names_used = std::collections::HashSet::new();
        let mut stack: Vec<usize> = roots.into_iter().rev().collect();
        let mut next = 0usize;
        while let Some(i) = stack.pop() {
            let raw = match nodes[i].get("name") {
                Some(Variant::String(s)) => s.replace([':', '/'], "_"),
                _ => String::new(),
            };
            let raw = if raw.is_empty() { "bone".to_string() } else { raw };
            let name = gen_unique_name(&mut names_used, &raw);
            let bone_parent = parent[i].and_then(|p| bone_of[p]).map_or(-1, |b| b as i64);
            props.insert(format!("bones/{next}/name"), Variant::String(name.clone()));
            props.insert(format!("bones/{next}/parent"), Variant::Int(bone_parent));
            props.insert(
                format!("bones/{next}/rest"),
                Variant::Transform3D(local_transform(&nodes[i])),
            );
            props.insert(format!("bones/{next}/enabled"), Variant::Bool(true));
            bone_of[i] = Some(next);
            bone_names[i] = name;
            next += 1;
            for &c in children[i].iter().rev() {
                if skel_of[c] == Some(k) {
                    stack.push(c);
                }
            }
        }
        skel_props.insert(k, props);
    }

    // -- the node arena -----------------------------------------------------
    let scene_name = match doc.get_property("scene_name") {
        Some(Variant::String(s)) if !s.is_empty() => s.clone(),
        _ => std::path::Path::new(&doc.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("GltfScene")
            .to_string(),
    };
    let mut arena = Arena::default();
    let root = arena.add(None, scene_name, "Node3D", HashMap::new());
    let mut skel_node: HashMap<usize, usize> = HashMap::new();
    let mut gltf_to_arena: HashMap<usize, usize> = HashMap::new();

    let skinned_mesh = |i: usize| int_of(&nodes[i], "skin").is_some() && int_of(&nodes[i], "mesh").is_some();

    // `_generate_scene_node`, iterative so a deep or hostile file cannot blow
    // the stack. Each entry: (glTF node, arena parent).
    let roots: Vec<usize> = match doc.get_property("scene_roots") {
        Some(Variant::Array(a)) => a
            .to_vec()
            .into_iter()
            .filter_map(|v| match v {
                Variant::Int(i) if i >= 0 && (i as usize) < n => Some(i as usize),
                _ => None,
            })
            .collect(),
        // A glTF may omit `scenes`: every node that is nobody's child is a root.
        _ => (0..n).filter(|&i| parent[i].is_none()).collect(),
    };
    let mut work: Vec<(usize, usize)> = roots.into_iter().rev().map(|i| (i, root)).collect();
    let mut visited = vec![false; n];
    while let Some((gi, at)) = work.pop() {
        if std::mem::replace(&mut visited[gi], true) {
            continue;
        }
        let dict = &nodes[gi];
        let mut pending: Vec<(usize, usize)> = Vec::new();
        if let Some(k) = skel_of[gi] {
            let sk = *skel_node.entry(k).or_insert_with(|| {
                arena.add(
                    Some(at),
                    skel_names[&k].clone(),
                    "Skeleton3D",
                    skel_props.remove(&k).unwrap_or_default(),
                )
            });
            let bone = bone_names[gi].clone();
            // A joint that also carries a mesh/camera/light gets that content
            // on a BoneAttachment3D (`requires_extra_node`).
            let class = node_class(dict, doc);
            if class != "Node3D" && !skinned_mesh(gi) {
                let att = arena.add_bone_attachment(sk, &bone);
                let content = arena.add(Some(att), bone.clone(), class, content_props(dict, doc));
                gltf_to_arena.insert(gi, content);
            }
            for &c in &children[gi] {
                if skel_of[c] == Some(k) || skinned_mesh(c) {
                    pending.push((c, sk));
                } else {
                    let att = arena.add_bone_attachment(sk, &bone);
                    pending.push((c, att));
                }
            }
        } else {
            let mut props = content_props(dict, doc);
            props.insert("transform".into(), Variant::Transform3D(local_transform(dict)));
            let id = arena.add(Some(at), node_names[gi].clone(), node_class(dict, doc), props);
            gltf_to_arena.insert(gi, id);
            for &c in &children[gi] {
                pending.push((c, id));
            }
        }
        // Reverse so siblings come off the stack in file order.
        work.extend(pending.into_iter().rev());
    }

    // -- skinned meshes move under their skeleton (`_process_mesh_instances`)
    for gi in 0..n {
        if !skinned_mesh(gi) {
            continue;
        }
        let (Some(&mesh), Some(skin)) = (gltf_to_arena.get(&gi), int_of(&nodes[gi], "skin")) else {
            continue;
        };
        let Some(skin_data) = skins.get(skin.max(0) as usize) else {
            continue;
        };
        let Some(&sk) = skin_to_skel
            .get(skin.max(0) as usize)
            .copied()
            .flatten()
            .and_then(|k| skel_node.get(&k))
        else {
            continue;
        };
        arena.reparent(mesh, sk);
        let props = &mut arena.nodes[mesh].props;
        props.insert("transform".into(), Variant::Transform3D(Transform3D::IDENTITY));
        props.insert(
            "skeleton".into(),
            Variant::NodePath(gdcore::node_path::NodePath::new("..")),
        );
        // The skin's binds, read positionally by the mesh's `bone_indices`
        // (`render_server_3d.rs` `authored_skin`). Without them the renderer
        // falls back to rest-derived binds — plausible, but not the rig.
        props.insert(
            "__skin__bind_count".into(),
            Variant::Int(skin_data.joints.len() as i64),
        );
        for (bi, &joint) in skin_data.joints.iter().enumerate() {
            props.insert(
                format!("__skin__bind/{bi}/name"),
                Variant::String(bone_names[joint].clone()),
            );
            let pose = skin_data
                .inverse_binds
                .get(bi)
                .copied()
                .unwrap_or(Transform3D::IDENTITY);
            props.insert(format!("__skin__bind/{bi}/pose"), Variant::Transform3D(pose));
        }
    }

    // -- clips --------------------------------------------------------------
    let mut targets: HashMap<i64, crate::animation::GltfTrackTarget> = HashMap::new();
    for gi in 0..n {
        let target = match (skel_of[gi], gltf_to_arena.get(&gi)) {
            (Some(k), _) => match skel_node.get(&k) {
                Some(&sk) => crate::animation::GltfTrackTarget::Bone {
                    skeleton: arena.path_of(sk),
                    bone: bone_names[gi].clone(),
                },
                None => continue,
            },
            (None, Some(&id)) => crate::animation::GltfTrackTarget::Node(arena.path_of(id)),
            (None, None) => continue,
        };
        targets.insert(gi as i64, target);
    }
    let animations = crate::animation::animations_from_gltf_resource_targeted(doc, &targets);
    if !animations.is_empty() {
        let name = arena.unique_child_name(root, "AnimationPlayer");
        arena.add(Some(root), name, "AnimationPlayer", HashMap::new());
    }

    Some(PackedScene::from_synthetic_nodes(arena.emit(), animations))
}

/// [`packed_scene_from_gltf`] for a `res://` (or filesystem) path: a mounted
/// pack first, then the file under the project root. `None` — reported — when
/// the file cannot be read or parsed.
pub fn packed_scene_from_gltf_path(path: &str) -> Option<PackedScene> {
    let doc = match gdcore::file_access::read_from_packs(path) {
        Some(bytes) => gdresource::import_gltf_bytes(&bytes, std::path::Path::new(path)),
        None => match gdcore::file_access::resolve_path(path) {
            Ok(fs) => gdresource::import_gltf(&fs),
            Err(e) => Err(e),
        },
    };
    let mut scene = match doc {
        Ok(doc) => packed_scene_from_gltf(&doc)?,
        Err(e) => {
            tracing::error!("glTF scene '{path}' could not be imported: {e}");
            return None;
        }
    };
    // Godot imports a scene THROUGH its `.import` sidecar: per-clip settings
    // such as `settings/loop_mode` live there, not in the .glb (pat-3mbf5).
    if let Ok(text) = gdcore::file_access::FileAccess::read_file_string(&format!("{path}.import")) {
        if let Some(clips) = scene.gltf_animations_mut() {
            apply_import_animation_settings(clips, &text);
        }
    }
    Some(scene)
}

/// Applies a scene `.import` sidecar's animation settings to the clips of an
/// imported glTF, in Godot's order (pat-3mbf5):
///
/// 1. with `nodes/use_name_suffixes` (default `true`), a name hint
///    `-loop`/`_loop`/`$loop` (also `loop_mode`, `cycle`) loops the clip and is
///    stripped from its name (`ResourceImporterScene::_pre_fix_node`);
/// 2. a clip listed under `_subresources` -> `"animations"` takes its
///    `settings/loop_mode` (0 none, 1 linear, 2 ping-pong), defaulting to 0 —
///    so an entry OVERRIDES both the glTF name rule and the hint, exactly as
///    `_post_fix_animations` fills absent options with their defaults.
///
/// ⚠️ A clip with no entry keeps what the glTF importer gave it. The official
/// demos loop `idle`/`run`/`walk` ONLY through this sidecar; their names match
/// no rule, so a project copied without its `.import` files does not loop them
/// in Godot either.
pub fn apply_import_animation_settings(clips: &mut [crate::animation::Animation], import_text: &str) {
    use crate::animation::LoopMode;

    let use_name_suffixes = !import_text
        .lines()
        .any(|l| l.trim().replace(' ', "") == "nodes/use_name_suffixes=false");
    if use_name_suffixes {
        for clip in clips.iter_mut() {
            for hint in ["loop_mode", "loop", "cycle"] {
                if import_name_has_hint(&clip.name, hint) {
                    clip.loop_mode = LoopMode::Linear;
                    clip.name = import_name_strip_hint(&clip.name, hint);
                }
            }
        }
    }

    let Some(Variant::Dictionary(sub)) = import_subresources(import_text) else {
        return;
    };
    let Some(Variant::Dictionary(anims)) = sub.get("animations") else {
        return;
    };
    for clip in clips.iter_mut() {
        let Some(Variant::Dictionary(settings)) = anims.get(clip.name.as_str()) else {
            continue;
        };
        clip.loop_mode = match settings.get("settings/loop_mode") {
            Some(Variant::Int(1)) => LoopMode::Linear,
            Some(Variant::Int(2)) => LoopMode::PingPong,
            _ => LoopMode::None,
        };
    }
}

/// The `[params] _subresources={...}` value of a `.import` sidecar, which
/// spans many lines and so is invisible to the line-based `.import` parser.
fn import_subresources(import_text: &str) -> Option<Variant> {
    let start = import_text.find("\n_subresources=")? + 1;
    let body = &import_text[start + "_subresources=".len()..];
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return gdresource::loader::parse_variant_value(&body[..=i]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// The importer's name with Blender's `.001`-style tail (digits, spaces,
/// underscores) removed — `_teststr`/`_fixstr`'s first step.
fn import_name_core(name: &str) -> &str {
    name.trim_end_matches(|c: char| c.is_ascii_digit() || (c as u32) <= 32 || c == '_')
}

/// Godot's `_teststr(name, hint)` (`resource_importer_scene.cpp`).
fn import_name_has_hint(name: &str, hint: &str) -> bool {
    let core = import_name_core(name).to_lowercase();
    core.contains(&format!("${hint}"))
        || core.ends_with(&format!("-{hint}"))
        || core.ends_with(&format!("_{hint}"))
}

/// Godot's `_fixstr(name, hint)`: the name with the hint removed, keeping the
/// stripped tail.
fn import_name_strip_hint(name: &str, hint: &str) -> String {
    let core = import_name_core(name);
    let end = &name[core.len()..];
    let dollar = format!("${hint}");
    if core.to_lowercase().contains(&dollar) {
        return format!("{}{end}", core.replace(&dollar, ""));
    }
    let lower = core.to_lowercase();
    if lower.ends_with(&format!("-{hint}")) || lower.ends_with(&format!("_{hint}")) {
        return format!("{}{end}", &core[..core.len() - hint.len() - 1]);
    }
    core.to_string()
}

/// Attaches the clips of an imported glTF scene to its `AnimationPlayer` node
/// (the first one among `own`, the nodes that scene level created).
///
/// `root_node` is Godot's default `..` — the imported scene's root, which every
/// track path (`Skeleton/Skeleton3D:hip`) is relative to.
pub(crate) fn attach_gltf_animation_player(
    tree: &mut SceneTree,
    own: &[NodeId],
    clips: &[crate::animation::Animation],
) {
    let Some(&player_node) = own.iter().find(|&&id| {
        tree.get_node(id)
            .is_some_and(|n| n.class_name() == "AnimationPlayer")
    }) else {
        return;
    };
    let mut player = crate::animation::AnimationPlayer::new();
    player.root_node = "..".to_string();
    for clip in clips {
        player.add_animation(clip.clone());
    }
    let _ = tree.attach_animation_player(player_node, player);
}

/// One skin of the document: its joints (glTF node indices, in bind order)
/// and the inverse bind matrix of each.
struct SkinData {
    joints: Vec<usize>,
    inverse_binds: Vec<Transform3D>,
}

fn read_skins(doc: &Resource, node_count: usize) -> Vec<SkinData> {
    let Some(Variant::Array(skins)) = doc.get_property("skins") else {
        return Vec::new();
    };
    skins
        .to_vec()
        .into_iter()
        .map(|skin| {
            let Variant::Dictionary(d) = skin else {
                return SkinData {
                    joints: Vec::new(),
                    inverse_binds: Vec::new(),
                };
            };
            let joints = match d.get("joints") {
                Some(Variant::Array(a)) => a
                    .to_vec()
                    .into_iter()
                    .filter_map(|v| match v {
                        Variant::Int(i) if i >= 0 && (i as usize) < node_count => Some(i as usize),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let inverse_binds = match d.get("inverse_bind_matrices") {
                Some(Variant::Array(a)) => a
                    .to_vec()
                    .into_iter()
                    .map(|m| {
                        let f: Vec<f32> = match m {
                            Variant::Array(m) => m
                                .to_vec()
                                .into_iter()
                                .map(|x| match x {
                                    Variant::Float(f) => f as f32,
                                    Variant::Int(i) => i as f32,
                                    _ => 0.0,
                                })
                                .collect(),
                            _ => Vec::new(),
                        };
                        if f.len() != 16 {
                            return Transform3D::IDENTITY;
                        }
                        // Column-major 4x4; `Basis` stores COLUMNS.
                        Transform3D {
                            basis: Basis {
                                x: Vector3::new(f[0], f[1], f[2]),
                                y: Vector3::new(f[4], f[5], f[6]),
                                z: Vector3::new(f[8], f[9], f[10]),
                            },
                            origin: Vector3::new(f[12], f[13], f[14]),
                        }
                    })
                    .collect(),
                _ => Vec::new(),
            };
            SkinData {
                joints,
                inverse_binds,
            }
        })
        .collect()
}

/// A glTF node's local TRS as a `Transform3D` (rotation is a quaternion).
fn local_transform(dict: &gdvariant::SharedDictionary) -> Transform3D {
    let t = floats(dict, "translation", 3).unwrap_or_else(|| vec![0.0, 0.0, 0.0]);
    let r = floats(dict, "rotation", 4).unwrap_or_else(|| vec![0.0, 0.0, 0.0, 1.0]);
    let s = floats(dict, "scale", 3).unwrap_or_else(|| vec![1.0, 1.0, 1.0]);
    let q = Quaternion::new(r[0], r[1], r[2], r[3]);
    Transform3D {
        basis: Basis::from_quaternion_scale(q, Vector3::new(s[0], s[1], s[2])),
        origin: Vector3::new(t[0], t[1], t[2]),
    }
}

/// The class-specific properties of a content node (a mesh's addressing).
fn content_props(dict: &gdvariant::SharedDictionary, doc: &Resource) -> HashMap<String, Variant> {
    let mut props = HashMap::new();
    if let Some(mesh_index) = int_of(dict, "mesh") {
        // `mesh` is what `node3d::get_mesh_path` reads; `gltf_mesh_index` is
        // what tells the render server WHICH mesh of the file this node uses.
        // Both are required — see the module header.
        props.insert("mesh".into(), Variant::String(doc.path.clone()));
        props.insert("gltf_mesh_index".into(), Variant::Int(mesh_index));
    }
    props
}

/// Godot's `String::validate_node_name`: the characters a NodePath reserves
/// become `_`.
fn validate_node_name(name: &str) -> String {
    name.chars()
        .map(|c| if matches!(c, '.' | ':' | '@' | '/' | '"' | '%') { '_' } else { c })
        .collect()
}

/// Godot's `GLTFDocument::_gen_unique_name`: `name`, then `name2`, `name3`…
fn gen_unique_name(used: &mut std::collections::HashSet<String>, name: &str) -> String {
    let mut index = 1;
    loop {
        let candidate = if index > 1 {
            format!("{name}{index}")
        } else {
            name.to_string()
        };
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

#[derive(Default)]
struct ArenaNode {
    name: String,
    class: String,
    parent: Option<usize>,
    children: Vec<usize>,
    props: HashMap<String, Variant>,
}

/// The imported scene under construction: plain indices, so a skinned mesh
/// can be reparented after the walk (as Godot does) before paths are fixed.
#[derive(Default)]
struct Arena {
    nodes: Vec<ArenaNode>,
}

impl Arena {
    fn add(
        &mut self,
        parent: Option<usize>,
        name: String,
        class: &str,
        props: HashMap<String, Variant>,
    ) -> usize {
        let id = self.nodes.len();
        self.nodes.push(ArenaNode {
            name,
            class: class.to_string(),
            parent,
            children: Vec::new(),
            props,
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        id
    }

    fn add_bone_attachment(&mut self, skeleton: usize, bone: &str) -> usize {
        let name = self.unique_child_name(skeleton, bone);
        let mut props = HashMap::new();
        props.insert("bone_name".to_string(), Variant::String(bone.to_string()));
        self.add(Some(skeleton), name, "BoneAttachment3D", props)
    }

    fn unique_child_name(&self, parent: usize, base: &str) -> String {
        let taken = |candidate: &str| {
            self.nodes[parent]
                .children
                .iter()
                .any(|&c| self.nodes[c].name == candidate)
        };
        if !taken(base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base}{i}"))
            .find(|c| !taken(c))
            .unwrap_or_else(|| base.to_string())
    }

    fn reparent(&mut self, id: usize, new_parent: usize) {
        if let Some(old) = self.nodes[id].parent {
            self.nodes[old].children.retain(|&c| c != id);
        }
        // Godot's `add_child(mi, true)`: a clash gets a readable suffix.
        let name = self.nodes[id].name.clone();
        let name = self.unique_child_name(new_parent, &name);
        self.nodes[id].name = name;
        self.nodes[id].parent = Some(new_parent);
        self.nodes[new_parent].children.push(id);
    }

    /// Path of `id` relative to the root (`"."` for the root itself).
    fn path_of(&self, id: usize) -> String {
        let mut parts = Vec::new();
        let mut cursor = id;
        while let Some(p) = self.nodes[cursor].parent {
            parts.push(self.nodes[cursor].name.as_str());
            cursor = p;
        }
        if parts.is_empty() {
            return ".".to_string();
        }
        parts.reverse();
        parts.join("/")
    }

    /// Depth-first, parents before children, as a `.tscn` lists its nodes.
    fn emit(mut self) -> Vec<SyntheticNode> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut stack = vec![0usize];
        while let Some(id) = stack.pop() {
            let parent_path = self.nodes[id].parent.map(|p| self.path_of(p));
            let node = std::mem::take(&mut self.nodes[id].props);
            out.push(SyntheticNode {
                name: self.nodes[id].name.clone(),
                class_name: self.nodes[id].class.clone(),
                parent_path,
                properties: node,
            });
            stack.extend(self.nodes[id].children.iter().rev().copied());
        }
        out
    }
}
