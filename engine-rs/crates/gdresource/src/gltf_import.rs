//! glTF 2.0 / GLB importer (pat-p0s82.5).
//!
//! Loads mesh hierarchy, materials, and node transforms into a
//! [`Resource`] that the 3D render path consumes via
//! `mesh3d_from_gltf_resource` / `material_from_gltf_resource`.
//!
//! # Supported
//! - ASCII `.gltf` (embedded `buffers[].uri` as `data:` base64 or external relative file)
//! - Binary `.glb` (JSON + BIN chunks)
//! - Mesh primitives: `POSITION`, optional `NORMAL`, `TEXCOORD_0`, `indices`
//! - Morph targets: `primitives[].targets` `POSITION` deltas, named from
//!   `mesh.extras.targetNames`
//! - Materials: `pbrMetallicRoughness` baseColor / metallic / roughness + emissive + doubleSided
//! - Nodes: name, children, mesh index, TRS or 4×4 matrix
//!
//! # Unsupported (documented)
//! - Morph-target `NORMAL` / `TANGENT` deltas (only `POSITION` is imported)
//! - Sparse accessors, Draco/meshopt compression
//! - Skinned mesh skinning matrices / animation playback
//! - Image / texture sampling (albedo texture URI is recorded but not decoded)
//! - Cameras, lights, extensions beyond core 2.0
//! - Multiple scenes (only default / first scene is imported)

use gdvariant::Dictionary;
use std::path::Path;
use std::sync::Arc;

use gdcore::error::{EngineError, EngineResult};
use gdcore::math::{Color, Vector3};
use gdvariant::Variant;
use serde_json::Value;

use crate::resource::Resource;

/// Public entry: import a `.gltf` or `.glb` file into a mesh document resource.
pub fn import_gltf(path: &Path) -> EngineResult<Arc<Resource>> {
    let bytes = std::fs::read(path).map_err(EngineError::Io)?;
    import_gltf_bytes(&bytes, path)
}

/// Import from raw file bytes. `path` is used for relative URI resolution and
/// the resource `path` field.
pub fn import_gltf_bytes(bytes: &[u8], path: &Path) -> EngineResult<Arc<Resource>> {
    if bytes.len() >= 12 && &bytes[0..4] == b"glTF" {
        import_glb(bytes, path)
    } else {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| EngineError::Parse(format!("gltf: invalid UTF-8: {e}")))?;
        let json: Value = serde_json::from_str(text)
            .map_err(|e| EngineError::Parse(format!("gltf: JSON parse error: {e}")))?;
        build_document(&json, path, None)
    }
}

fn import_glb(bytes: &[u8], path: &Path) -> EngineResult<Arc<Resource>> {
    if bytes.len() < 12 {
        return Err(EngineError::Parse("glb: header too short".into()));
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != 0x4654_6C67 {
        return Err(EngineError::Parse(format!(
            "glb: bad magic 0x{magic:08X}, expected glTF"
        )));
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != 2 {
        return Err(EngineError::Unsupported(format!(
            "glb: only glTF 2.0 supported (got version {version})"
        )));
    }
    let total_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if total_len > bytes.len() {
        return Err(EngineError::Parse(format!(
            "glb: declared length {total_len} > file {}",
            bytes.len()
        )));
    }

    let mut offset = 12usize;
    let mut json_text: Option<String> = None;
    let mut bin_blob: Option<Vec<u8>> = None;

    while offset + 8 <= total_len {
        let chunk_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let chunk_type = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        offset += 8;
        if offset + chunk_len > bytes.len() {
            return Err(EngineError::Parse("glb: chunk overruns file".into()));
        }
        let chunk = &bytes[offset..offset + chunk_len];
        offset += chunk_len;
        // Align to 4 bytes
        offset = (offset + 3) & !3;

        match chunk_type {
            0x4E4F_534A => {
                // JSON
                let s = std::str::from_utf8(chunk)
                    .map_err(|e| EngineError::Parse(format!("glb JSON chunk: {e}")))?;
                json_text = Some(
                    s.trim_end_matches(|c: char| c == '\0' || c == ' ')
                        .to_string(),
                );
            }
            0x004E_4942 => {
                // BIN
                bin_blob = Some(chunk.to_vec());
            }
            other => {
                tracing::debug!(chunk_type = other, "glb: skipping unknown chunk");
            }
        }
    }

    let json_str = json_text.ok_or_else(|| EngineError::Parse("glb: missing JSON chunk".into()))?;
    let json: Value = serde_json::from_str(&json_str)
        .map_err(|e| EngineError::Parse(format!("glb JSON parse: {e}")))?;
    build_document(&json, path, bin_blob.as_deref())
}

fn build_document(
    json: &Value,
    path: &Path,
    glb_bin: Option<&[u8]>,
) -> EngineResult<Arc<Resource>> {
    let buffers = load_buffers(json, path, glb_bin)?;

    // Godot imports a .glb/.gltf through ResourceImporterScene, whose
    // `get_resource_type()` is "PackedScene" — so `load("res://m.glb")` hands a
    // script a PackedScene and `s is PackedScene` is true. Reporting
    // "GltfDocument" made `can_instantiate` false and `instantiate()` raise
    // "cannot instantiate a GltfDocument resource" (pat-mgixk.23).
    //
    // ⚠️ The class is the only thing that changes. The parsed document — nodes,
    // per-primitive subresources, animations — still lives on this same
    // resource, and `gltf_document` marks it as that kind of PackedScene so a
    // consumer can still tell the two apart rather than inferring from the path.
    let mut doc = Resource::new("PackedScene");
    doc.set_property("gltf_document", Variant::Bool(true));
    doc.path = path
        .to_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.display().to_string());

    // --- materials + textures/images ---
    let materials = json
        .get("materials")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let textures = json
        .get("textures")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let images = json
        .get("images")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // --- meshes / primitives → subresources ---
    let meshes = json
        .get("meshes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut mesh_count = 0i64;
    let mut prim_total = 0i64;

    for (mi, mesh) in meshes.iter().enumerate() {
        let mesh_name = mesh
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Mesh")
            .to_string();
        let primitives = mesh
            .get("primitives")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if primitives.is_empty() {
            continue;
        }
        mesh_count += 1;

        for (pi, prim) in primitives.iter().enumerate() {
            let mut sub = Resource::new("ArrayMeshSurface");
            sub.set_property("mesh_index", Variant::Int(mi as i64));
            sub.set_property("primitive_index", Variant::Int(pi as i64));
            sub.set_property("mesh_name", Variant::String(mesh_name.clone()));

            let attrs = prim
                .get("attributes")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();

            // POSITION (required)
            let pos_acc = attrs
                .get("POSITION")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    EngineError::Parse(format!("gltf: mesh {mi} prim {pi} missing POSITION"))
                })? as usize;
            let positions = read_vec3_accessor(json, &buffers, pos_acc)?;
            if positions.is_empty() {
                return Err(EngineError::Parse(format!(
                    "gltf: mesh {mi} prim {pi} has empty POSITION"
                )));
            }
            sub.set_property(
                "vertices",
                Variant::Array(positions.iter().map(|p| Variant::Vector3(*p)).collect()),
            );

            // NORMAL (optional)
            if let Some(n_acc) = attrs.get("NORMAL").and_then(|v| v.as_u64()) {
                let normals = read_vec3_accessor(json, &buffers, n_acc as usize)?;
                sub.set_property(
                    "normals",
                    Variant::Array(normals.iter().map(|n| Variant::Vector3(*n)).collect()),
                );
            }

            // TEXCOORD_0 (optional)
            if let Some(t_acc) = attrs.get("TEXCOORD_0").and_then(|v| v.as_u64()) {
                let uvs = read_vec2_accessor(json, &buffers, t_acc as usize)?;
                let uv_variants: Vec<Variant> = uvs
                    .iter()
                    .map(|uv| {
                        Variant::Array(
                            (vec![Variant::Float(uv[0] as f64), Variant::Float(uv[1] as f64)])
                                .into(),
                        )
                    })
                    .collect();
                sub.set_property("uvs", Variant::Array(uv_variants.into()));
            }

            // JOINTS_0 / WEIGHTS_0 (optional) — per-vertex skinning influences
            // (pat-0q2kx.3). Godot's GLTFDocument imports these into
            // ARRAY_BONES / ARRAY_WEIGHTS; without them a rigged mesh has no
            // influences and a Skeleton3D can never deform it. Stored flattened
            // (4 components per vertex) alongside the other vertex arrays.
            if let Some(j_acc) = attrs.get("JOINTS_0").and_then(|v| v.as_u64()) {
                let joints = read_vec4_index_accessor(json, &buffers, j_acc as usize)?;
                sub.set_property(
                    "bone_indices",
                    Variant::Array(
                        joints
                            .iter()
                            .flatten()
                            .map(|&c| Variant::Int(c as i64))
                            .collect(),
                    ),
                );
            }
            if let Some(w_acc) = attrs.get("WEIGHTS_0").and_then(|v| v.as_u64()) {
                let weights = read_vec4_float_accessor(json, &buffers, w_acc as usize)?;
                sub.set_property(
                    "bone_weights",
                    Variant::Array(
                        weights
                            .iter()
                            .flatten()
                            .map(|&c| Variant::Float(c as f64))
                            .collect(),
                    ),
                );
            }

            // Morph targets (glTF `primitives[].targets`) — the per-vertex
            // POSITION deltas Godot imports as blend shapes
            // (`modules/gltf/gltf_document.cpp:1784-1833`). Names come from
            // `mesh.extras.targetNames`, falling back to Godot's `morph_<k>`
            // for an unnamed or empty entry. Without this the importer dropped
            // them silently, so `gdrender3d::morph` — which has carried the
            // blending maths and its unit tests all along — had no path by
            // which any file could reach it (pat-s3rrv.23).
            //
            // ⚠️ Stored as DELTAS, which is what the accessor holds. Godot
            // converts to absolute positions here
            // (`w_varr[l] = r_varr[l] + r_src_varr[l]`) because its ArrayMesh
            // blend-shape format wants absolutes; `apply_morph` wants deltas
            // and adds them to the base itself. Converting would add the base
            // twice and move every morphed vertex to roughly double its
            // position — which reads as a scale bug, not as a bad import.
            //
            // ⚠️ A target accessor shorter than POSITION is left short rather
            // than padded. `apply_morph` only touches the vertices a delta
            // array covers, which is the same result as Godot padding with the
            // base position: a base position IS a zero delta.
            if let Some(targets) = prim.get("targets").and_then(|v| v.as_array()) {
                if !targets.is_empty() {
                    let target_names = mesh
                        .get("extras")
                        .and_then(|e| e.get("targetNames"))
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let mut names: Vec<Variant> = Vec::with_capacity(targets.len());
                    let mut deltas: Vec<Variant> = Vec::with_capacity(targets.len());
                    for (k, target) in targets.iter().enumerate() {
                        let name = target_names
                            .get(k)
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .unwrap_or_else(|| format!("morph_{k}"));
                        let d = match target.get("POSITION").and_then(|v| v.as_u64()) {
                            Some(acc) => read_vec3_accessor(json, &buffers, acc as usize)?,
                            None => Vec::new(),
                        };
                        names.push(Variant::String(name));
                        deltas.push(Variant::Array(
                            d.iter().map(|p| Variant::Vector3(*p)).collect(),
                        ));
                    }
                    sub.set_property("blend_shape_names", Variant::Array(names.into()));
                    sub.set_property("blend_shape_deltas", Variant::Array(deltas.into()));
                }
            }

            // indices (optional — if missing, sequential triangle list)
            //
            // 🔑 glTF winds front faces COUNTER-clockwise; Godot's are CLOCKWISE
            // (`doc/classes/ArrayMesh.xml:49`), so Godot's importer swaps the
            // last two indices of every triangle — for an index buffer
            // (`gltf_document.cpp:1486-1493`, "convert ccw to cw for front face")
            // and for a generated one alike (`:1700-1712`). Without it every
            // imported model is inside-out under back-face culling. Only for
            // TRIANGLES (`mode` 4, the glTF default).
            let triangles = prim.get("mode").and_then(|v| v.as_u64()).unwrap_or(4) == 4;
            let mut indices: Vec<i64> =
                if let Some(i_acc) = prim.get("indices").and_then(|v| v.as_u64()) {
                    read_index_accessor(json, &buffers, i_acc as usize)?
                        .iter()
                        .map(|&i| i as i64)
                        .collect()
                } else {
                    (0..positions.len() as i64).collect()
                };
            if triangles {
                for tri in indices.chunks_exact_mut(3) {
                    tri.swap(1, 2);
                }
            }
            sub.set_property(
                "indices",
                Variant::Array(indices.into_iter().map(Variant::Int).collect()),
            );

            // material
            if let Some(mat_idx) = prim.get("material").and_then(|v| v.as_u64()) {
                let mi_u = mat_idx as usize;
                if let Some(mat) = materials.get(mi_u) {
                    apply_material_props(&mut sub, mat, &textures, &images);
                }
                sub.set_property("material_index", Variant::Int(mat_idx as i64));
            }

            let key = format!("mesh_{mi}_prim_{pi}");
            doc.subresources.insert(key, Arc::new(sub));
            prim_total += 1;
        }
    }

    // --- nodes (hierarchy + transforms) ---
    let nodes = json
        .get("nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut node_dicts: Vec<Variant> = Vec::with_capacity(nodes.len());
    for (ni, node) in nodes.iter().enumerate() {
        let mut d = Dictionary::new();
        d.insert("index", Variant::Int(ni as i64));
        let name = node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Node")
            .to_string();
        d.insert("name", Variant::String(name));
        if let Some(mesh) = node.get("mesh").and_then(|v| v.as_u64()) {
            d.insert("mesh", Variant::Int(mesh as i64));
        }
        if let Some(camera) = node.get("camera").and_then(|v| v.as_u64()) {
            d.insert("camera", Variant::Int(camera as i64));
        }
        // A skinned mesh node names its skin; Godot reparents such a mesh
        // under the skin's generated Skeleton3D (pat-m2jgd).
        if let Some(skin) = node.get("skin").and_then(|v| v.as_u64()) {
            d.insert("skin", Variant::Int(skin as i64));
        }
        // KHR_lights_punctual: node references a light via extensions.
        if let Some(light) = node
            .get("extensions")
            .and_then(|e| e.get("KHR_lights_punctual"))
            .and_then(|l| l.get("light"))
            .and_then(|v| v.as_u64())
        {
            d.insert("light", Variant::Int(light as i64));
        }
        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            let kids: Vec<Variant> = children
                .iter()
                .filter_map(|c| c.as_u64().map(|u| Variant::Int(u as i64)))
                .collect();
            d.insert("children", Variant::Array(kids.into()));
        }
        let (t, r, s) = node_trs(node);
        d.insert(
            "translation",
            Variant::Array(
                (vec![
                    Variant::Float(t[0] as f64),
                    Variant::Float(t[1] as f64),
                    Variant::Float(t[2] as f64),
                ])
                .into(),
            ),
        );
        d.insert(
            "rotation",
            Variant::Array(
                (vec![
                    Variant::Float(r[0] as f64),
                    Variant::Float(r[1] as f64),
                    Variant::Float(r[2] as f64),
                    Variant::Float(r[3] as f64),
                ])
                .into(),
            ),
        );
        d.insert(
            "scale",
            Variant::Array(
                (vec![
                    Variant::Float(s[0] as f64),
                    Variant::Float(s[1] as f64),
                    Variant::Float(s[2] as f64),
                ])
                .into(),
            ),
        );
        node_dicts.push(Variant::Dictionary(d.into()));
    }
    doc.set_property("nodes", Variant::Array(node_dicts.into()));
    doc.set_property("node_count", Variant::Int(nodes.len() as i64));
    doc.set_property("mesh_count", Variant::Int(mesh_count));
    doc.set_property("primitive_count", Variant::Int(prim_total));

    // --- cameras (core glTF) ---
    if let Some(cameras) = json.get("cameras").and_then(|v| v.as_array()) {
        let cam_dicts: Vec<Variant> = cameras
            .iter()
            .map(|c| Variant::Dictionary(camera_dict(c).into()))
            .collect();
        doc.set_property("camera_count", Variant::Int(cam_dicts.len() as i64));
        doc.set_property("cameras", Variant::Array(cam_dicts.into()));
    }

    // --- lights (KHR_lights_punctual) ---
    if let Some(lights) = json
        .get("extensions")
        .and_then(|e| e.get("KHR_lights_punctual"))
        .and_then(|l| l.get("lights"))
        .and_then(|v| v.as_array())
    {
        let light_dicts: Vec<Variant> = lights
            .iter()
            .map(|l| Variant::Dictionary(light_dict(l).into()))
            .collect();
        doc.set_property("light_count", Variant::Int(light_dicts.len() as i64));
        doc.set_property("lights", Variant::Array(light_dicts.into()));
    }
    doc.set_property(
        "unsupported_extensions",
        Variant::Array(
            UNSUPPORTED_EXTENSIONS
                .iter()
                .map(|s| Variant::String((*s).into()))
                .collect(),
        ),
    );

    // default scene roots
    let scene_idx = json.get("scene").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    if let Some(scenes) = json.get("scenes").and_then(|v| v.as_array()) {
        if let Some(sc) = scenes.get(scene_idx) {
            if let Some(roots) = sc.get("nodes").and_then(|v| v.as_array()) {
                let root_ids: Vec<Variant> = roots
                    .iter()
                    .filter_map(|n| n.as_u64().map(|u| Variant::Int(u as i64)))
                    .collect();
                doc.set_property("scene_roots", Variant::Array(root_ids.into()));
            }
            // The default scene's NAME, which this handler used to read past
            // while taking only its `nodes` (pat-mgixk.23). Godot names the
            // imported scene's root after the glTF scene
            // (`GLTFDocument::_generate_scene_node` /
            // `ResourceImporterScene::import`), and `gltf_scene`'s
            // `build_gltf_subtree` already PREFERS this property over the file
            // stem — it just always read `None`, so every instanced glTF root
            // was named after the file instead of the asset's own scene.
            //
            // Skipped when absent or empty rather than publishing `""`, because
            // the builder's `!s.is_empty()` guard is what hands it back to the
            // file-stem fallback; a glTF is allowed to omit `scenes[i].name`.
            if let Some(name) = sc.get("name").and_then(|v| v.as_str()) {
                if !name.is_empty() {
                    doc.set_property("scene_name", Variant::String(name.to_string()));
                }
            }
        }
    }

    // --- animations (pat-0q2kx.155) ---
    //
    // `parse_animations` already produced this data and *nothing consumed it*:
    // the clips never reached the imported scene. Publish it on the document so
    // the scene layer can build `Animation`s from it. The conversion cannot live
    // here — `Animation`/`AnimationPlayer` are `gdscene` types and `gdscene`
    // depends on this crate, so the dependency only runs one way.
    //
    // Deliberately stored as document PROPERTIES, not subresources:
    // `mesh3d_from_gltf_resource` sorts `subresources` by key and treats
    // `subs[0]` as the primary surface, aborting on a missing `vertices`. An
    // `animation_*` key sorts before `mesh_0_prim_0`, so adding clips as
    // subresources would silently break mesh loading for every animated .glb.
    // --- skins (pat-m2jgd) ---
    //
    // `parse_skins` had no non-test caller, so an imported scene could not
    // know which nodes are JOINTS. Godot's importer turns a skin's joints into
    // the bones of a generated `Skeleton3D` (`gltf_document.cpp`
    // `_determine_skeletons` / `_create_skeletons`) — that node is what a
    // scene with editable children addresses as `Player/Skeleton/Skeleton3D`.
    // Published as plain properties for the same reason as the animations
    // below: `subresources` are mesh primitives.
    let skins = parse_skins(json, &buffers)?;
    if !skins.is_empty() {
        let skin_dicts: Vec<Variant> = skins
            .iter()
            .map(|skin| {
                let mut d = Dictionary::new();
                d.insert(
                    "joints",
                    Variant::Array(
                        skin.joints
                            .iter()
                            .map(|j| Variant::Int(*j as i64))
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                );
                // One 16-float column-major matrix per joint, in joint order.
                d.insert(
                    "inverse_bind_matrices",
                    Variant::Array(
                        skin.inverse_bind_matrices
                            .iter()
                            .map(|m| {
                                Variant::Array(
                                    m.iter()
                                        .map(|f| Variant::Float(*f as f64))
                                        .collect::<Vec<_>>()
                                        .into(),
                                )
                            })
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                );
                if let Some(root) = skin.skeleton_root {
                    d.insert("skeleton", Variant::Int(root as i64));
                }
                Variant::Dictionary(d.into())
            })
            .collect();
        doc.set_property("skins", Variant::Array(skin_dicts.into()));
    }

    let animations = parse_animations(json, &buffers)?;
    if !animations.is_empty() {
        let clips: Vec<Variant> = animations
            .iter()
            .map(|anim| {
                let mut clip = Dictionary::new();
                clip.insert("name", Variant::String(anim.name.clone()));
                clip.insert("length", Variant::Float(anim.length as f64));
                let channels: Vec<Variant> = anim
                    .channels
                    .iter()
                    .map(|ch| {
                        let mut c = Dictionary::new();
                        c.insert("target_node", Variant::Int(ch.target_node as i64));
                        c.insert(
                            "path",
                            Variant::String(
                                match ch.path {
                                    GltfAnimationPath::Translation => "translation",
                                    GltfAnimationPath::Rotation => "rotation",
                                    GltfAnimationPath::Scale => "scale",
                                    GltfAnimationPath::Weights => "weights",
                                }
                                .to_string(),
                            ),
                        );
                        c.insert(
                            "interpolation",
                            Variant::String(
                                match ch.interpolation {
                                    GltfInterpolation::Linear => "linear",
                                    GltfInterpolation::Step => "step",
                                    GltfInterpolation::CubicSpline => "cubicspline",
                                }
                                .to_string(),
                            ),
                        );
                        // Needed to slice `values`: 3 for translation/scale, 4 for
                        // rotation, and one per morph target for weights.
                        c.insert("components", Variant::Int(ch.components as i64));
                        c.insert(
                            "times",
                            Variant::Array(
                                ch.times.iter().map(|t| Variant::Float(*t as f64)).collect(),
                            ),
                        );
                        c.insert(
                            "values",
                            Variant::Array(
                                ch.values
                                    .iter()
                                    .map(|v| Variant::Float(*v as f64))
                                    .collect(),
                            ),
                        );
                        Variant::Dictionary(c.into())
                    })
                    .collect();
                clip.insert("channels", Variant::Array(channels.into()));
                Variant::Dictionary(clip.into())
            })
            .collect();
        doc.set_property("animation_count", Variant::Int(clips.len() as i64));
        doc.set_property("animations", Variant::Array(clips.into()));
    }

    Ok(Arc::new(doc))
}

/// Extensions we intentionally do not implement yet.
const UNSUPPORTED_EXTENSIONS: &[&str] = &[
    "KHR_draco_mesh_compression",
    "EXT_meshopt_compression",
    "KHR_materials_transmission",
    "KHR_materials_volume",
    "KHR_animation_pointer",
];

fn apply_material_props(sub: &mut Resource, mat: &Value, textures: &[Value], images: &[Value]) {
    let pbr = mat.get("pbrMetallicRoughness");
    let albedo = pbr
        .and_then(|p| p.get("baseColorFactor"))
        .and_then(|v| v.as_array())
        .map(|a| {
            Color::new(
                a.first().and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                a.get(1).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                a.get(2).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                a.get(3).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
            )
        })
        .unwrap_or(Color::WHITE);
    sub.set_property("material_albedo", Variant::Color(albedo));

    let metallic = pbr
        .and_then(|p| p.get("metallicFactor"))
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;
    let roughness = pbr
        .and_then(|p| p.get("roughnessFactor"))
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;
    sub.set_property("material_metallic", Variant::Float(metallic as f64));
    sub.set_property("material_roughness", Variant::Float(roughness as f64));

    let emissive = mat
        .get("emissiveFactor")
        .and_then(|v| v.as_array())
        .map(|a| {
            Color::new(
                a.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                a.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                a.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                1.0,
            )
        })
        .unwrap_or(Color::BLACK);
    sub.set_property("material_emissive", Variant::Color(emissive));

    let double_sided = mat
        .get("doubleSided")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    sub.set_property("material_double_sided", Variant::Bool(double_sided));

    // alphaMode: OPAQUE (default) / MASK / BLEND, with alphaCutoff for MASK.
    let alpha_mode = mat
        .get("alphaMode")
        .and_then(|v| v.as_str())
        .unwrap_or("OPAQUE")
        .to_string();
    sub.set_property("material_alpha_mode", Variant::String(alpha_mode.clone()));
    if alpha_mode == "MASK" {
        let cutoff = mat
            .get("alphaCutoff")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        sub.set_property("material_alpha_cutoff", Variant::Float(cutoff));
    }

    // Texture maps: resolve each `<map>.index` through textures -> images and
    // record the resolved image URI so the render path can load the pixels.
    let map = |name: &str| {
        mat.get(name)
            .and_then(|v| v.get("index"))
            .and_then(|v| v.as_u64())
    };
    // baseColorTexture/metallicRoughnessTexture live under pbrMetallicRoughness.
    let pbr_map = |name: &str| {
        pbr.and_then(|p| p.get(name))
            .and_then(|v| v.get("index"))
            .and_then(|v| v.as_u64())
    };
    for (prop, idx) in [
        ("material_albedo_texture", pbr_map("baseColorTexture")),
        (
            "material_metallic_roughness_texture",
            pbr_map("metallicRoughnessTexture"),
        ),
        ("material_normal_texture", map("normalTexture")),
        ("material_emissive_texture", map("emissiveTexture")),
        ("material_occlusion_texture", map("occlusionTexture")),
    ] {
        let Some(tex_idx) = idx else { continue };
        sub.set_property(format!("{prop}_index"), Variant::Int(tex_idx as i64));
        if let Some(uri) = resolve_texture_uri(textures, images, tex_idx as usize) {
            sub.set_property(prop, Variant::String(uri));
        }
    }

    if let Some(name) = mat.get("name").and_then(|v| v.as_str()) {
        sub.set_property("material_name", Variant::String(name.into()));
    }
}

/// Resolves a glTF texture index to its backing image URI, following
/// `textures[i].source -> images[source].uri`. Returns `None` when the texture
/// or its image source is missing (e.g. GLB `bufferView`-backed images, which
/// carry no URI).
fn resolve_texture_uri(textures: &[Value], images: &[Value], tex_idx: usize) -> Option<String> {
    let source = textures
        .get(tex_idx)?
        .get("source")
        .and_then(|v| v.as_u64())? as usize;
    let image = images.get(source)?;
    image
        .get("uri")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Builds a node-dictionary entry for a core glTF camera. Perspective cameras
/// carry `yfov`/`aspect`/`znear`/`zfar`; orthographic carry `xmag`/`ymag`.
fn camera_dict(cam: &Value) -> Dictionary {
    let mut d = Dictionary::new();
    let cam_type = cam
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("perspective");
    d.insert("type", Variant::String(cam_type.to_string()));
    if let Some(name) = cam.get("name").and_then(|v| v.as_str()) {
        d.insert("name", Variant::String(name.to_string()));
    }
    let getf =
        |obj: Option<&Value>, key: &str| obj.and_then(|o| o.get(key)).and_then(|v| v.as_f64());
    if cam_type == "orthographic" {
        let ortho = cam.get("orthographic");
        if let Some(v) = getf(ortho, "xmag") {
            d.insert("xmag", Variant::Float(v));
        }
        if let Some(v) = getf(ortho, "ymag") {
            d.insert("ymag", Variant::Float(v));
        }
        if let Some(v) = getf(ortho, "znear") {
            d.insert("znear", Variant::Float(v));
        }
        if let Some(v) = getf(ortho, "zfar") {
            d.insert("zfar", Variant::Float(v));
        }
    } else {
        let persp = cam.get("perspective");
        if let Some(v) = getf(persp, "yfov") {
            d.insert("yfov", Variant::Float(v));
        }
        if let Some(v) = getf(persp, "aspectRatio") {
            d.insert("aspect", Variant::Float(v));
        }
        if let Some(v) = getf(persp, "znear") {
            d.insert("znear", Variant::Float(v));
        }
        if let Some(v) = getf(persp, "zfar") {
            d.insert("zfar", Variant::Float(v));
        }
    }
    d
}

/// Builds a dictionary entry for a KHR_lights_punctual light. Carries
/// `type` (directional/point/spot), `color`, `intensity`, optional `range`,
/// and spot cone angles for spot lights.
fn light_dict(light: &Value) -> Dictionary {
    let mut d = Dictionary::new();
    let light_type = light
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("point");
    d.insert("type", Variant::String(light_type.to_string()));
    if let Some(name) = light.get("name").and_then(|v| v.as_str()) {
        d.insert("name", Variant::String(name.to_string()));
    }
    let color = light
        .get("color")
        .and_then(|v| v.as_array())
        .map(|a| {
            Color::new(
                a.first().and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                a.get(1).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                a.get(2).and_then(|x| x.as_f64()).unwrap_or(1.0) as f32,
                1.0,
            )
        })
        .unwrap_or(Color::WHITE);
    d.insert("color", Variant::Color(color));
    let intensity = light
        .get("intensity")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    d.insert("intensity", Variant::Float(intensity));
    if let Some(range) = light.get("range").and_then(|v| v.as_f64()) {
        d.insert("range", Variant::Float(range));
    }
    if light_type == "spot" {
        let spot = light.get("spot");
        let inner = spot
            .and_then(|s| s.get("innerConeAngle"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let outer = spot
            .and_then(|s| s.get("outerConeAngle"))
            .and_then(|v| v.as_f64())
            .unwrap_or(std::f64::consts::FRAC_PI_4);
        d.insert("spot_inner_cone_angle", Variant::Float(inner));
        d.insert("spot_outer_cone_angle", Variant::Float(outer));
    }
    d
}

fn node_trs(node: &Value) -> ([f32; 3], [f32; 4], [f32; 3]) {
    // matrix takes precedence over TRS if present (glTF spec)
    if let Some(m) = node.get("matrix").and_then(|v| v.as_array()) {
        if m.len() >= 16 {
            let f = |i: usize| m[i].as_f64().unwrap_or(0.0) as f32;
            // column-major 4x4: translation is m[12], m[13], m[14]
            let t = [f(12), f(13), f(14)];
            // scale from basis-column lengths
            let sx = (f(0) * f(0) + f(1) * f(1) + f(2) * f(2)).sqrt();
            let sy = (f(4) * f(4) + f(5) * f(5) + f(6) * f(6)).sqrt();
            let sz = (f(8) * f(8) + f(9) * f(9) + f(10) * f(10)).sqrt();
            let s = [sx.max(1e-8), sy.max(1e-8), sz.max(1e-8)];
            // rotation: normalize the basis columns to remove scale, then
            // convert the pure-rotation 3x3 to a quaternion.
            let r = matrix_rotation_quat(&f, s);
            return (t, r, s);
        }
    }
    let t = read_f32_3(node.get("translation"), [0.0, 0.0, 0.0]);
    let r = read_f32_4(node.get("rotation"), [0.0, 0.0, 0.0, 1.0]);
    let s = read_f32_3(node.get("scale"), [1.0, 1.0, 1.0]);
    (t, r, s)
}

/// Extracts the rotation quaternion `[x, y, z, w]` from a column-major glTF node
/// matrix. `f(i)` reads matrix element `i`; `scale` is the already-computed
/// per-axis basis-column length used to divide out scale before conversion.
///
/// The 3x3 rotation is built by normalizing each basis column, then converted
/// with the standard trace-based algorithm. `m[row][col]` below follows
/// column-major storage: column 0 is `f(0..=2)`, column 1 `f(4..=6)`,
/// column 2 `f(8..=10)`.
fn matrix_rotation_quat<F>(f: &F, scale: [f32; 3]) -> [f32; 4]
where
    F: Fn(usize) -> f32,
{
    let (sx, sy, sz) = (scale[0], scale[1], scale[2]);
    // Row-major access into the normalized rotation basis: m[row][col].
    let m00 = f(0) / sx;
    let m10 = f(1) / sx;
    let m20 = f(2) / sx;
    let m01 = f(4) / sy;
    let m11 = f(5) / sy;
    let m21 = f(6) / sy;
    let m02 = f(8) / sz;
    let m12 = f(9) / sz;
    let m22 = f(10) / sz;

    let trace = m00 + m11 + m22;
    let (x, y, z, w) = if trace > 0.0 {
        let s = 0.5 / (trace + 1.0).sqrt();
        ((m21 - m12) * s, (m02 - m20) * s, (m10 - m01) * s, 0.25 / s)
    } else if m00 > m11 && m00 > m22 {
        let s = 2.0 * (1.0 + m00 - m11 - m22).sqrt();
        (0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s)
    } else if m11 > m22 {
        let s = 2.0 * (1.0 + m11 - m00 - m22).sqrt();
        ((m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s)
    } else {
        let s = 2.0 * (1.0 + m22 - m00 - m11).sqrt();
        ((m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s)
    };

    // Normalize to guard against accumulated float error.
    let n = (x * x + y * y + z * z + w * w).sqrt();
    if n > 1e-8 {
        [x / n, y / n, z / n, w / n]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

fn read_f32_3(v: Option<&Value>, default: [f32; 3]) -> [f32; 3] {
    let Some(arr) = v.and_then(|x| x.as_array()) else {
        return default;
    };
    [
        arr.first()
            .and_then(|x| x.as_f64())
            .unwrap_or(default[0] as f64) as f32,
        arr.get(1)
            .and_then(|x| x.as_f64())
            .unwrap_or(default[1] as f64) as f32,
        arr.get(2)
            .and_then(|x| x.as_f64())
            .unwrap_or(default[2] as f64) as f32,
    ]
}

fn read_f32_4(v: Option<&Value>, default: [f32; 4]) -> [f32; 4] {
    let Some(arr) = v.and_then(|x| x.as_array()) else {
        return default;
    };
    [
        arr.first()
            .and_then(|x| x.as_f64())
            .unwrap_or(default[0] as f64) as f32,
        arr.get(1)
            .and_then(|x| x.as_f64())
            .unwrap_or(default[1] as f64) as f32,
        arr.get(2)
            .and_then(|x| x.as_f64())
            .unwrap_or(default[2] as f64) as f32,
        arr.get(3)
            .and_then(|x| x.as_f64())
            .unwrap_or(default[3] as f64) as f32,
    ]
}

// ---------------------------------------------------------------------------
// Buffers / accessors
// ---------------------------------------------------------------------------

fn load_buffers(json: &Value, path: &Path, glb_bin: Option<&[u8]>) -> EngineResult<Vec<Vec<u8>>> {
    let Some(bufs) = json.get("buffers").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(bufs.len());
    for (i, buf) in bufs.iter().enumerate() {
        let byte_length = buf.get("byteLength").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        if let Some(uri) = buf.get("uri").and_then(|v| v.as_str()) {
            if let Some(b64) = uri.strip_prefix("data:") {
                // data:application/octet-stream;base64,....
                let data = b64
                    .split(',')
                    .nth(1)
                    .ok_or_else(|| EngineError::Parse("gltf: bad data URI".into()))?;
                let decoded = base64_decode(data)?;
                out.push(decoded);
            } else {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                let file_path = parent.join(uri);
                let data = std::fs::read(&file_path).map_err(EngineError::Io)?;
                out.push(data);
            }
        } else if i == 0 {
            // GLB: first buffer without URI is the BIN chunk
            let bin = glb_bin.ok_or_else(|| {
                EngineError::Parse("gltf: buffer 0 has no uri and no GLB BIN chunk".into())
            })?;
            let mut data = bin.to_vec();
            if data.len() < byte_length {
                return Err(EngineError::Parse(format!(
                    "glb BIN too short: {} < {byte_length}",
                    data.len()
                )));
            }
            data.truncate(byte_length.max(data.len()).min(data.len()));
            // Prefer declared length when smaller
            if data.len() > byte_length && byte_length > 0 {
                data.truncate(byte_length);
            }
            out.push(data);
        } else {
            return Err(EngineError::Parse(format!("gltf: buffer {i} missing uri")));
        }
    }
    Ok(out)
}

fn accessor_meta(json: &Value, accessor_idx: usize) -> EngineResult<AccessorMeta> {
    let accessors = json
        .get("accessors")
        .and_then(|v| v.as_array())
        .ok_or_else(|| EngineError::Parse("gltf: missing accessors".into()))?;
    let acc = accessors
        .get(accessor_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: accessor {accessor_idx} out of range")))?;
    let count = acc
        .get("count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| EngineError::Parse("gltf: accessor missing count".into()))?
        as usize;
    let component_type = acc
        .get("componentType")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| EngineError::Parse("gltf: accessor missing componentType".into()))?
        as u32;
    let type_str = acc
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("SCALAR")
        .to_string();
    let byte_offset = acc.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    let bv_idx = acc
        .get("bufferView")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| EngineError::Parse("gltf: accessor missing bufferView".into()))?
        as usize;
    let views = json
        .get("bufferViews")
        .and_then(|v| v.as_array())
        .ok_or_else(|| EngineError::Parse("gltf: missing bufferViews".into()))?;
    let view = views
        .get(bv_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: bufferView {bv_idx} OOR")))?;
    let buffer_idx = view.get("buffer").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let view_offset = view.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let view_length = view.get("byteLength").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let stride = view
        .get("byteStride")
        .and_then(|v| v.as_u64())
        .map(|s| s as usize);

    Ok(AccessorMeta {
        count,
        component_type,
        type_str,
        buffer_idx,
        byte_offset: view_offset + byte_offset,
        view_length,
        stride,
    })
}

struct AccessorMeta {
    count: usize,
    component_type: u32,
    type_str: String,
    buffer_idx: usize,
    byte_offset: usize,
    view_length: usize,
    stride: Option<usize>,
}

fn component_size(ct: u32) -> EngineResult<usize> {
    Ok(match ct {
        5120 | 5121 => 1, // BYTE / UNSIGNED_BYTE
        5122 | 5123 => 2, // SHORT / UNSIGNED_SHORT
        5125 | 5126 => 4, // UNSIGNED_INT / FLOAT
        other => {
            return Err(EngineError::Unsupported(format!(
                "gltf: componentType {other}"
            )))
        }
    })
}

fn type_components(t: &str) -> usize {
    match t {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" => 4,
        "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        _ => 1,
    }
}

fn read_vec3_accessor(
    json: &Value,
    buffers: &[Vec<u8>],
    accessor_idx: usize,
) -> EngineResult<Vec<Vector3>> {
    let meta = accessor_meta(json, accessor_idx)?;
    if meta.type_str != "VEC3" {
        return Err(EngineError::Parse(format!(
            "gltf: expected VEC3 accessor, got {}",
            meta.type_str
        )));
    }
    if meta.component_type != 5126 {
        return Err(EngineError::Unsupported(
            "gltf: only FLOAT VEC3 accessors supported for positions/normals".into(),
        ));
    }
    let buf = buffers
        .get(meta.buffer_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx)))?;
    let elem_size = 12usize;
    let stride = meta.stride.unwrap_or(elem_size);
    let mut out = Vec::with_capacity(meta.count);
    for i in 0..meta.count {
        let off = meta.byte_offset + i * stride;
        if off + 12 > buf.len() {
            return Err(EngineError::Parse(format!(
                "gltf: VEC3 accessor overrun at element {i}"
            )));
        }
        let x = f32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let y = f32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap());
        let z = f32::from_le_bytes(buf[off + 8..off + 12].try_into().unwrap());
        out.push(Vector3::new(x, y, z));
    }
    let _ = meta.view_length; // validated via overrun checks
    Ok(out)
}

fn read_vec2_accessor(
    json: &Value,
    buffers: &[Vec<u8>],
    accessor_idx: usize,
) -> EngineResult<Vec<[f32; 2]>> {
    let meta = accessor_meta(json, accessor_idx)?;
    if meta.type_str != "VEC2" || meta.component_type != 5126 {
        return Err(EngineError::Unsupported(
            "gltf: only FLOAT VEC2 for TEXCOORD_0".into(),
        ));
    }
    let buf = buffers
        .get(meta.buffer_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx)))?;
    let elem_size = 8usize;
    let stride = meta.stride.unwrap_or(elem_size);
    let mut out = Vec::with_capacity(meta.count);
    for i in 0..meta.count {
        let off = meta.byte_offset + i * stride;
        if off + 8 > buf.len() {
            return Err(EngineError::Parse("gltf: VEC2 overrun".into()));
        }
        let u = f32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let v = f32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap());
        out.push([u, v]);
    }
    Ok(out)
}

/// Reads a `VEC4` FLOAT accessor — glTF `WEIGHTS_0` (pat-0q2kx.3).
fn read_vec4_float_accessor(
    json: &Value,
    buffers: &[Vec<u8>],
    accessor_idx: usize,
) -> EngineResult<Vec<[f32; 4]>> {
    let meta = accessor_meta(json, accessor_idx)?;
    if meta.type_str != "VEC4" {
        return Err(EngineError::Parse(format!(
            "gltf: expected VEC4 accessor, got {}",
            meta.type_str
        )));
    }
    if meta.component_type != 5126 {
        // glTF also allows normalized u8/u16 weights; those are not produced by
        // the exporters this importer targets yet.
        return Err(EngineError::Unsupported(
            "gltf: only FLOAT VEC4 supported for WEIGHTS_0".into(),
        ));
    }
    let buf = buffers
        .get(meta.buffer_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx)))?;
    let elem_size = 16usize;
    let stride = meta.stride.unwrap_or(elem_size);
    let mut out = Vec::with_capacity(meta.count);
    for i in 0..meta.count {
        let off = meta.byte_offset + i * stride;
        if off + elem_size > buf.len() {
            return Err(EngineError::Parse(format!(
                "gltf: VEC4 float accessor overrun at element {i}"
            )));
        }
        let mut v = [0f32; 4];
        for (c, slot) in v.iter_mut().enumerate() {
            let o = off + c * 4;
            *slot = f32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
        }
        out.push(v);
    }
    Ok(out)
}

/// Reads a `VEC4` integer accessor — glTF `JOINTS_0`, whose components are
/// UNSIGNED_BYTE or UNSIGNED_SHORT (pat-0q2kx.3).
fn read_vec4_index_accessor(
    json: &Value,
    buffers: &[Vec<u8>],
    accessor_idx: usize,
) -> EngineResult<Vec<[u32; 4]>> {
    let meta = accessor_meta(json, accessor_idx)?;
    if meta.type_str != "VEC4" {
        return Err(EngineError::Parse(format!(
            "gltf: expected VEC4 accessor, got {}",
            meta.type_str
        )));
    }
    let cs = component_size(meta.component_type)?;
    let buf = buffers
        .get(meta.buffer_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx)))?;
    let elem_size = cs * 4;
    let stride = meta.stride.unwrap_or(elem_size);
    let mut out = Vec::with_capacity(meta.count);
    for i in 0..meta.count {
        let off = meta.byte_offset + i * stride;
        if off + elem_size > buf.len() {
            return Err(EngineError::Parse(format!(
                "gltf: VEC4 index accessor overrun at element {i}"
            )));
        }
        let mut v = [0u32; 4];
        for (c, slot) in v.iter_mut().enumerate() {
            let o = off + c * cs;
            *slot = match meta.component_type {
                5121 => buf[o] as u32,
                5123 => u16::from_le_bytes(buf[o..o + 2].try_into().unwrap()) as u32,
                5125 => u32::from_le_bytes(buf[o..o + 4].try_into().unwrap()),
                other => {
                    return Err(EngineError::Unsupported(format!(
                        "gltf: JOINTS_0 componentType {other}"
                    )))
                }
            };
        }
        out.push(v);
    }
    Ok(out)
}

fn read_index_accessor(
    json: &Value,
    buffers: &[Vec<u8>],
    accessor_idx: usize,
) -> EngineResult<Vec<u32>> {
    let meta = accessor_meta(json, accessor_idx)?;
    let comps = type_components(&meta.type_str);
    if comps != 1 {
        return Err(EngineError::Parse("gltf: indices must be SCALAR".into()));
    }
    let cs = component_size(meta.component_type)?;
    let buf = buffers
        .get(meta.buffer_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx)))?;
    let stride = meta.stride.unwrap_or(cs);
    let mut out = Vec::with_capacity(meta.count);
    for i in 0..meta.count {
        let off = meta.byte_offset + i * stride;
        if off + cs > buf.len() {
            return Err(EngineError::Parse("gltf: index overrun".into()));
        }
        let idx = match meta.component_type {
            5121 => buf[off] as u32,
            5123 => u16::from_le_bytes(buf[off..off + 2].try_into().unwrap()) as u32,
            5125 => u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()),
            other => {
                return Err(EngineError::Unsupported(format!(
                    "gltf: index componentType {other}"
                )))
            }
        };
        out.push(idx);
    }
    Ok(out)
}

// Minimal base64 decoder (no extra crate).
fn base64_decode(input: &str) -> EngineResult<Vec<u8>> {
    fn val(c: u8) -> EngineResult<u8> {
        Ok(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => {
                return Err(EngineError::Parse(format!(
                    "gltf: invalid base64 char {:?}",
                    c as char
                )))
            }
        })
    }
    let clean: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    let mut i = 0;
    while i + 4 <= clean.len() {
        let n = ((val(clean[i])? as u32) << 18)
            | ((val(clean[i + 1])? as u32) << 12)
            | ((val(clean[i + 2])? as u32) << 6)
            | (val(clean[i + 3])? as u32);
        out.push(((n >> 16) & 0xFF) as u8);
        out.push(((n >> 8) & 0xFF) as u8);
        out.push((n & 0xFF) as u8);
        i += 4;
    }
    let rem = clean.len() - i;
    if rem == 2 {
        let n = ((val(clean[i])? as u32) << 18) | ((val(clean[i + 1])? as u32) << 12);
        out.push(((n >> 16) & 0xFF) as u8);
    } else if rem == 3 {
        let n = ((val(clean[i])? as u32) << 18)
            | ((val(clean[i + 1])? as u32) << 12)
            | ((val(clean[i + 2])? as u32) << 6);
        out.push(((n >> 16) & 0xFF) as u8);
        out.push(((n >> 8) & 0xFF) as u8);
    } else if rem == 1 {
        return Err(EngineError::Parse("gltf: truncated base64".into()));
    }
    // Handle padding: if input ended with =, trim last bytes
    let pad = input.bytes().filter(|&b| b == b'=').count();
    if pad == 1 && !out.is_empty() {
        out.pop();
    } else if pad == 2 && out.len() >= 2 {
        out.pop();
        out.pop();
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Skins & animations (pat-u3l8c.88)
//
// Parses the glTF `skins` and `animations` arrays into structured data that
// higher layers turn into a `Skeleton3D` (bone hierarchy + bind poses) and an
// `AnimationPlayer` clip. Reuses the accessor/buffer plumbing above. Building
// the actual scene nodes (gdscene) and per-vertex JOINTS_0/WEIGHTS_0 skinning
// deformation are separate follow-ups.
// ---------------------------------------------------------------------------

/// A parsed glTF skin — the joint node list, bone parent hierarchy, and bind
/// poses that build a `Skeleton3D`.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfSkin {
    /// Node indices of the joints, in skin-joint (bone) order.
    pub joints: Vec<usize>,
    /// Parent bone index (into `joints`) for each joint, or `None` for a root
    /// bone. Derived from the node `children` graph.
    pub parents: Vec<Option<usize>>,
    /// Inverse bind matrix (column-major 4×4, 16 floats) per joint. Empty when
    /// the skin omits `inverseBindMatrices` (identity implied).
    pub inverse_bind_matrices: Vec<[f32; 16]>,
    /// Optional common skeleton root node index (`skin.skeleton`).
    pub skeleton_root: Option<usize>,
}

/// The node property an animation channel drives (Godot `Animation` track).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GltfAnimationPath {
    /// Local translation (VEC3 output).
    Translation,
    /// Local rotation quaternion (VEC4 xyzw output).
    Rotation,
    /// Local scale (VEC3 output).
    Scale,
    /// Morph-target weights (SCALAR output, one per target per key).
    Weights,
}

/// Keyframe interpolation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GltfInterpolation {
    /// Linear interpolation (glTF default).
    Linear,
    /// Stepwise / constant-hold interpolation.
    Step,
    /// Cubic spline — output holds in-tangent, value, and out-tangent per key.
    CubicSpline,
}

/// One animation channel: a keyframe track for a single node property.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfAnimationChannel {
    /// Target node index.
    pub target_node: usize,
    /// Property this track drives.
    pub path: GltfAnimationPath,
    /// Interpolation mode.
    pub interpolation: GltfInterpolation,
    /// Keyframe times in seconds (sampler `input`).
    pub times: Vec<f32>,
    /// Flattened keyframe values (sampler `output`): `components` floats per key
    /// (×3 keys for `CubicSpline`).
    pub values: Vec<f32>,
    /// Float components per keyframe value (3 translation/scale, 4 rotation, …).
    pub components: usize,
}

/// A parsed glTF animation → a Godot `AnimationPlayer` clip.
#[derive(Debug, Clone, PartialEq)]
pub struct GltfAnimation {
    /// Clip name (defaults to `animation_<i>`).
    pub name: String,
    /// Channels (tracks) in the clip.
    pub channels: Vec<GltfAnimationChannel>,
    /// Clip length in seconds (max keyframe time across channels).
    pub length: f32,
}

/// Reads a `FLOAT` accessor with `expected_comps` components per element into a
/// flat `Vec<f32>` of `count * expected_comps` values. Handles `SCALAR` (times),
/// `VEC3`/`VEC4` (translation/scale/rotation), and `MAT4` (bind matrices).
fn read_flat_f32_accessor(
    json: &Value,
    buffers: &[Vec<u8>],
    accessor_idx: usize,
    expected_comps: usize,
) -> EngineResult<Vec<f32>> {
    let meta = accessor_meta(json, accessor_idx)?;
    if meta.component_type != 5126 {
        return Err(EngineError::Unsupported(format!(
            "gltf: expected FLOAT accessor, got componentType {}",
            meta.component_type
        )));
    }
    let comps = type_components(&meta.type_str);
    if comps != expected_comps {
        return Err(EngineError::Parse(format!(
            "gltf: accessor {accessor_idx} has {comps} components ({}), expected {expected_comps}",
            meta.type_str
        )));
    }
    let buf = buffers
        .get(meta.buffer_idx)
        .ok_or_else(|| EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx)))?;
    let elem_size = comps * 4;
    let stride = meta.stride.unwrap_or(elem_size);
    let mut out = Vec::with_capacity(meta.count * comps);
    for i in 0..meta.count {
        let base = meta.byte_offset + i * stride;
        if base + elem_size > buf.len() {
            return Err(EngineError::Parse(format!(
                "gltf: FLOAT accessor overrun at element {i}"
            )));
        }
        for c in 0..comps {
            let off = base + c * 4;
            out.push(f32::from_le_bytes(buf[off..off + 4].try_into().unwrap()));
        }
    }
    Ok(out)
}

/// For each joint, finds its parent joint — the joint whose node lists this
/// joint's node among its `children` — returning an index into `joints` or
/// `None` for a root bone.
fn compute_joint_parents(joints: &[usize], nodes: Option<&Vec<Value>>) -> Vec<Option<usize>> {
    let mut parents: Vec<Option<usize>> = vec![None; joints.len()];
    let Some(nodes) = nodes else {
        return parents;
    };
    for (ji, &node_idx) in joints.iter().enumerate() {
        for (pi, &parent_node) in joints.iter().enumerate() {
            if pi == ji {
                continue;
            }
            let Some(node) = nodes.get(parent_node) else {
                continue;
            };
            if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
                if children.iter().any(|c| c.as_u64() == Some(node_idx as u64)) {
                    parents[ji] = Some(pi);
                    break;
                }
            }
        }
    }
    parents
}

/// Parses the glTF `skins` array into [`GltfSkin`]s (empty when absent).
pub fn parse_skins(json: &Value, buffers: &[Vec<u8>]) -> EngineResult<Vec<GltfSkin>> {
    let Some(skins) = json.get("skins").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let nodes = json.get("nodes").and_then(|v| v.as_array());
    let mut out = Vec::with_capacity(skins.len());
    for skin in skins {
        let joints: Vec<usize> = skin
            .get("joints")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|j| j.as_u64().map(|n| n as usize))
                    .collect()
            })
            .unwrap_or_default();

        let inverse_bind_matrices = match skin.get("inverseBindMatrices").and_then(|v| v.as_u64()) {
            Some(acc) => read_flat_f32_accessor(json, buffers, acc as usize, 16)?
                .chunks_exact(16)
                .map(|c| {
                    let mut m = [0.0f32; 16];
                    m.copy_from_slice(c);
                    m
                })
                .collect(),
            None => Vec::new(),
        };

        let skeleton_root = skin
            .get("skeleton")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let parents = compute_joint_parents(&joints, nodes);

        out.push(GltfSkin {
            joints,
            parents,
            inverse_bind_matrices,
            skeleton_root,
        });
    }
    Ok(out)
}

/// Parses the glTF `animations` array into [`GltfAnimation`]s (empty when
/// absent). Channels whose target lacks a node or uses an unrecognised path are
/// skipped rather than erroring.
pub fn parse_animations(json: &Value, buffers: &[Vec<u8>]) -> EngineResult<Vec<GltfAnimation>> {
    let Some(anims) = json.get("animations").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(anims.len());
    for (ai, anim) in anims.iter().enumerate() {
        let name = anim
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("animation_{ai}"));
        let samplers = anim
            .get("samplers")
            .and_then(|v| v.as_array())
            .ok_or_else(|| EngineError::Parse("gltf: animation missing samplers".into()))?;
        let channels_json = anim
            .get("channels")
            .and_then(|v| v.as_array())
            .ok_or_else(|| EngineError::Parse("gltf: animation missing channels".into()))?;

        let mut channels = Vec::new();
        let mut length = 0.0f32;
        for ch in channels_json {
            let sampler_idx = ch
                .get("sampler")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| EngineError::Parse("gltf: channel missing sampler".into()))?
                as usize;
            let Some(target) = ch.get("target") else {
                continue;
            };
            let Some(target_node) = target
                .get("node")
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
            else {
                continue; // channel with no node target (e.g. pointer ext) — skip
            };
            let path = match target.get("path").and_then(|v| v.as_str()).unwrap_or("") {
                "translation" => GltfAnimationPath::Translation,
                "rotation" => GltfAnimationPath::Rotation,
                "scale" => GltfAnimationPath::Scale,
                "weights" => GltfAnimationPath::Weights,
                _ => continue, // unknown path — skip
            };
            let sampler = samplers
                .get(sampler_idx)
                .ok_or_else(|| EngineError::Parse("gltf: sampler index out of range".into()))?;
            let input_acc = sampler
                .get("input")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| EngineError::Parse("gltf: sampler missing input".into()))?
                as usize;
            let output_acc = sampler
                .get("output")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| EngineError::Parse("gltf: sampler missing output".into()))?
                as usize;
            let interpolation = match sampler
                .get("interpolation")
                .and_then(|v| v.as_str())
                .unwrap_or("LINEAR")
            {
                "STEP" => GltfInterpolation::Step,
                "CUBICSPLINE" => GltfInterpolation::CubicSpline,
                _ => GltfInterpolation::Linear,
            };

            let times = read_flat_f32_accessor(json, buffers, input_acc, 1)?;
            let out_meta = accessor_meta(json, output_acc)?;
            let components = type_components(&out_meta.type_str);
            let values = read_flat_f32_accessor(json, buffers, output_acc, components)?;
            if let Some(&t) = times.last() {
                if t > length {
                    length = t;
                }
            }
            channels.push(GltfAnimationChannel {
                target_node,
                path,
                interpolation,
                times,
                values,
                components,
            });
        }
        out.push(GltfAnimation {
            name,
            channels,
            length,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector3;

    #[test]
    fn node_matrix_transform_preserves_rotation() {
        use serde_json::json;
        // Column-major 4x4 for a pure 90° rotation about +Y (no scale/translation).
        // Basis X -> -Z, Y -> Y, Z -> +X.
        let node = json!({
            "matrix": [
                0.0, 0.0, -1.0, 0.0,
                0.0, 1.0,  0.0, 0.0,
                1.0, 0.0,  0.0, 0.0,
                0.0, 0.0,  0.0, 1.0
            ]
        });
        let (t, r, s) = node_trs(&node);
        assert!(t.iter().all(|c| c.abs() < 1e-5), "translation zero: {t:?}");
        for c in s {
            assert!((c - 1.0).abs() < 1e-5, "unit scale: {s:?}");
        }
        // Expected quaternion for 90° about Y: [x,y,z,w] = [0, sin45, 0, cos45].
        let s45 = std::f32::consts::FRAC_1_SQRT_2;
        let expected = [0.0, s45, 0.0, s45];
        for (got, exp) in r.iter().zip(expected.iter()) {
            assert!(
                (got - exp).abs() < 1e-4,
                "rotation quat mismatch: got {r:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn node_matrix_transform_recovers_rotation_and_scale() {
        use serde_json::json;
        // 90° about +Y with non-uniform scale (2, 3, 4) applied per basis column.
        // Column i = scale_i * rotated_basis_i.
        let node = json!({
            "matrix": [
                0.0, 0.0, -2.0, 0.0,
                0.0, 3.0,  0.0, 0.0,
                4.0, 0.0,  0.0, 0.0,
                5.0, 6.0,  7.0, 1.0
            ]
        });
        let (t, r, s) = node_trs(&node);
        assert!(
            (t[0] - 5.0).abs() < 1e-5 && (t[1] - 6.0).abs() < 1e-5 && (t[2] - 7.0).abs() < 1e-5
        );
        assert!((s[0] - 2.0).abs() < 1e-5, "sx: {s:?}");
        assert!((s[1] - 3.0).abs() < 1e-5, "sy: {s:?}");
        assert!((s[2] - 4.0).abs() < 1e-5, "sz: {s:?}");
        // Rotation must be recovered independent of the non-uniform scale.
        let s45 = std::f32::consts::FRAC_1_SQRT_2;
        let expected = [0.0, s45, 0.0, s45];
        for (got, exp) in r.iter().zip(expected.iter()) {
            assert!(
                (got - exp).abs() < 1e-4,
                "quat: got {r:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn material_imports_textures_and_alpha_mode() {
        use serde_json::json;
        let textures = vec![json!({ "source": 0 }), json!({ "source": 1 })];
        let images = vec![
            json!({ "uri": "albedo.png" }),
            json!({ "uri": "normal.png" }),
        ];
        let mat = json!({
            "name": "Tex",
            "pbrMetallicRoughness": {
                "baseColorTexture": { "index": 0 }
            },
            "normalTexture": { "index": 1 },
            "alphaMode": "MASK",
            "alphaCutoff": 0.25
        });
        let mut sub = Resource::new("ArrayMeshSurface");
        apply_material_props(&mut sub, &mat, &textures, &images);
        assert_eq!(
            sub.get_property("material_albedo_texture"),
            Some(&Variant::String("albedo.png".to_string()))
        );
        assert_eq!(
            sub.get_property("material_albedo_texture_index"),
            Some(&Variant::Int(0))
        );
        assert_eq!(
            sub.get_property("material_normal_texture"),
            Some(&Variant::String("normal.png".to_string()))
        );
        assert_eq!(
            sub.get_property("material_alpha_mode"),
            Some(&Variant::String("MASK".to_string()))
        );
        assert_eq!(
            sub.get_property("material_alpha_cutoff"),
            Some(&Variant::Float(0.25))
        );
    }

    #[test]
    fn camera_and_light_nodes_imported() {
        use serde_json::json;
        // No meshes/buffers — just scene graph with a camera node + a light node.
        let json = json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0, 1] }],
            "extensions": {
                "KHR_lights_punctual": {
                    "lights": [
                        { "type": "spot", "color": [1.0, 0.5, 0.0], "intensity": 3.0,
                          "spot": { "innerConeAngle": 0.1, "outerConeAngle": 0.5 } }
                    ]
                }
            },
            "cameras": [
                { "type": "perspective", "perspective": { "yfov": 0.8, "znear": 0.1, "zfar": 100.0 } }
            ],
            "nodes": [
                { "name": "Cam", "camera": 0 },
                { "name": "Lamp", "extensions": { "KHR_lights_punctual": { "light": 0 } } }
            ]
        });
        // Serialize as a .gltf (JSON) so import parses cameras/lights.
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("cl.gltf");
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        let res = import_gltf(&path).expect("import cameras+lights");

        assert_eq!(res.get_property("camera_count"), Some(&Variant::Int(1)));
        assert_eq!(res.get_property("light_count"), Some(&Variant::Int(1)));

        // node dicts carry camera/light references.
        let nodes = match res.get_property("nodes") {
            Some(Variant::Array(a)) => a,
            other => panic!("nodes: {other:?}"),
        };
        let cam_node = match nodes.to_vec()[0].clone() {
            Variant::Dictionary(d) => d,
            other => panic!("cam node: {other:?}"),
        };
        assert_eq!(cam_node.get("camera"), Some(Variant::Int(0)));
        let lamp_node = match nodes.to_vec()[1].clone() {
            Variant::Dictionary(d) => d,
            other => panic!("lamp node: {other:?}"),
        };
        assert_eq!(lamp_node.get("light"), Some(Variant::Int(0)));

        // light dict carries type + cone angles.
        let lights = match res.get_property("lights") {
            Some(Variant::Array(a)) => a,
            other => panic!("lights: {other:?}"),
        };
        let light = match lights.to_vec()[0].clone() {
            Variant::Dictionary(d) => d,
            other => panic!("light: {other:?}"),
        };
        assert_eq!(light.get("type"), Some(Variant::String("spot".to_string())));
        assert_eq!(light.get("intensity"), Some(Variant::Float(3.0)));
        assert_eq!(
            light.get("spot_outer_cone_angle"),
            Some(Variant::Float(0.5))
        );
    }

    fn make_triangle_glb() -> Vec<u8> {
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]];
        let indices: [u16; 3] = [0, 1, 2];

        let mut bin = Vec::new();
        for p in &positions {
            for &v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for n in &normals {
            for &v in n {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for &i in &indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{
                "name": "TriNode",
                "mesh": 0,
                "translation": [1.0, 2.0, 3.0]
            }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0,  "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 36, "target": 34962 },
                { "buffer": 0, "byteOffset": 72, "byteLength": 6,  "target": 34963 }
            ],
            "accessors": [
                {
                    "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                    "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0]
                },
                { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" },
                { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "materials": [{
                "name": "RedMetal",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [1.0, 0.0, 0.0, 1.0],
                    "metallicFactor": 0.8,
                    "roughnessFactor": 0.3
                },
                "emissiveFactor": [0.1, 0.0, 0.0],
                "doubleSided": true
            }],
            "meshes": [{
                "name": "Triangle",
                "primitives": [{
                    "attributes": { "POSITION": 0, "NORMAL": 1 },
                    "indices": 2,
                    "material": 0
                }]
            }]
        });

        let mut json_bytes = serde_json::to_vec(&json).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }
        let total_length = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut glb = Vec::with_capacity(total_length);
        glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total_length as u32).to_le_bytes());
        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        glb.extend_from_slice(&json_bytes);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);
        glb
    }

    #[test]
    fn import_glb_mesh_vertices_and_material() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("tri.glb");
        std::fs::write(&path, make_triangle_glb()).unwrap();

        let res = import_gltf(&path).expect("import_gltf must succeed");
        // Godot's scene importer reports PackedScene, which is what makes
        // `load(..) is PackedScene` and `instantiate()` work (pat-mgixk.23).
        assert_eq!(res.class_name, "PackedScene");
        assert_eq!(
            res.get_property("gltf_document"),
            Some(&Variant::Bool(true)),
            "still marked as the glTF flavour of PackedScene",
        );
        assert_eq!(res.get_property("mesh_count"), Some(&Variant::Int(1)));
        assert_eq!(res.get_property("node_count"), Some(&Variant::Int(1)));

        let sub = res
            .subresources
            .get("mesh_0_prim_0")
            .expect("mesh_0_prim_0 subresource");
        let verts = match sub.get_property("vertices") {
            Some(Variant::Array(a)) => a,
            other => panic!("vertices missing: {other:?}"),
        };
        assert_eq!(verts.len(), 3);
        assert_eq!(
            verts.to_vec()[0],
            Variant::Vector3(Vector3::new(0.0, 0.0, 0.0))
        );
        assert_eq!(
            verts.to_vec()[1],
            Variant::Vector3(Vector3::new(1.0, 0.0, 0.0))
        );
        assert_eq!(
            verts.to_vec()[2],
            Variant::Vector3(Vector3::new(0.0, 1.0, 0.0))
        );

        match sub.get_property("material_albedo") {
            Some(Variant::Color(c)) => {
                assert!((c.r - 1.0).abs() < 1e-5);
                assert!((c.g - 0.0).abs() < 1e-5);
                assert!((c.b - 0.0).abs() < 1e-5);
            }
            other => panic!("material_albedo: {other:?}"),
        }
        match sub.get_property("material_metallic") {
            Some(Variant::Float(f)) => assert!((*f - 0.8).abs() < 1e-5, "metallic={f}"),
            other => panic!("metallic: {other:?}"),
        }
        match sub.get_property("material_roughness") {
            Some(Variant::Float(f)) => assert!((*f - 0.3).abs() < 1e-5, "roughness={f}"),
            other => panic!("roughness: {other:?}"),
        }
        assert_eq!(
            sub.get_property("material_double_sided"),
            Some(&Variant::Bool(true))
        );

        // Node transform
        let nodes = match res.get_property("nodes") {
            Some(Variant::Array(a)) => a,
            _ => panic!("nodes"),
        };
        let node0 = match nodes.to_vec()[0].clone() {
            Variant::Dictionary(d) => d,
            _ => panic!("node dict"),
        };
        assert_eq!(node0.get("name"), Some(Variant::String("TriNode".into())));
        assert_eq!(node0.get("mesh"), Some(Variant::Int(0)));
        match node0.get("translation") {
            Some(Variant::Array(t)) => {
                assert_eq!(t.to_vec()[0], Variant::Float(1.0));
                assert_eq!(t.to_vec()[1], Variant::Float(2.0));
                assert_eq!(t.to_vec()[2], Variant::Float(3.0));
            }
            other => panic!("translation: {other:?}"),
        }
    }

    #[test]
    fn import_gltf_json_with_data_uri_buffer() {
        // Build positions as base64 (3 * 3 * 4 = 36 bytes of zeros + simple triangle)
        let mut bin = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]] {
            for v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            let mut i = 0;
            while i + 3 <= bin.len() {
                let n = ((bin[i] as u32) << 16) | ((bin[i + 1] as u32) << 8) | (bin[i + 2] as u32);
                s.push(T[((n >> 18) & 63) as usize] as char);
                s.push(T[((n >> 12) & 63) as usize] as char);
                s.push(T[((n >> 6) & 63) as usize] as char);
                s.push(T[(n & 63) as usize] as char);
                i += 3;
            }
            s
        };
        let json = format!(
            r#"{{
              "asset": {{"version": "2.0"}},
              "scenes": [{{"nodes": [0]}}],
              "nodes": [{{"name": "Root", "mesh": 0}}],
              "buffers": [{{"byteLength": 36, "uri": "data:application/octet-stream;base64,{b64}"}}],
              "bufferViews": [{{"buffer": 0, "byteOffset": 0, "byteLength": 36}}],
              "accessors": [{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}}],
              "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}}}]}}]
            }}"#
        );
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("simple.gltf");
        std::fs::write(&path, json).unwrap();

        let res = import_gltf(&path).expect("gltf json import");
        assert_eq!(res.get_property("mesh_count"), Some(&Variant::Int(1)));
        let sub = res.subresources.get("mesh_0_prim_0").unwrap();
        let verts = match sub.get_property("vertices") {
            Some(Variant::Array(a)) => a,
            _ => panic!("no verts"),
        };
        assert_eq!(verts.len(), 3);
        assert_eq!(
            verts.to_vec()[1],
            Variant::Vector3(Vector3::new(2.0, 0.0, 0.0))
        );
    }

    /// glTF `JOINTS_0` / `WEIGHTS_0` reach the imported surface as flattened
    /// per-vertex quads (pat-0q2kx.3). Without them a rigged mesh arrives with
    /// no influences and no `Skeleton3D` can ever deform it.
    #[test]
    fn import_gltf_carries_joints_and_weights() {
        // 2 vertices. Vertex 0 rides joint 0, vertex 1 splits 0.75/0.25 between
        // joints 1 and 2 — enough to prove component order survives.
        let mut bin = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0]] {
            for v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        assert_eq!(bin.len(), 24);
        // JOINTS_0: UNSIGNED_SHORT VEC4 (the other common exporter width).
        for j in [[0u16, 0, 0, 0], [1, 2, 0, 0]] {
            for c in j {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        assert_eq!(bin.len(), 40);
        // WEIGHTS_0: FLOAT VEC4.
        for w in [[1.0f32, 0.0, 0.0, 0.0], [0.75, 0.25, 0.0, 0.0]] {
            for c in w {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        assert_eq!(bin.len(), 72);

        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0,  "byteLength": 24 },
                { "buffer": 0, "byteOffset": 24, "byteLength": 16 },
                { "buffer": 0, "byteOffset": 40, "byteLength": 32 }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3" },
                { "bufferView": 1, "componentType": 5123, "count": 2, "type": "VEC4" },
                { "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC4" }
            ],
            "meshes": [{
                "primitives": [{
                    "attributes": { "POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2 }
                }]
            }]
        });
        let mut json_bytes = serde_json::to_vec(&json).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }
        let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut glb = Vec::with_capacity(total);
        glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        glb.extend_from_slice(&json_bytes);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        glb.extend_from_slice(&bin);

        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("rigged.glb");
        std::fs::write(&path, glb).unwrap();

        let res = import_gltf(&path).expect("rigged glb import");
        let sub = res.subresources.get("mesh_0_prim_0").expect("prim");

        match sub.get_property("bone_indices") {
            Some(Variant::Array(a)) => {
                let got: Vec<i64> = a
                    .to_vec()
                    .iter()
                    .map(|v| match v {
                        Variant::Int(i) => *i,
                        other => panic!("bone index not an int: {other:?}"),
                    })
                    .collect();
                assert_eq!(got, vec![0, 0, 0, 0, 1, 2, 0, 0]);
            }
            other => panic!("bone_indices missing: {other:?}"),
        }
        match sub.get_property("bone_weights") {
            Some(Variant::Array(a)) => {
                let got: Vec<f64> = a
                    .to_vec()
                    .iter()
                    .map(|v| match v {
                        Variant::Float(f) => *f,
                        other => panic!("bone weight not a float: {other:?}"),
                    })
                    .collect();
                assert_eq!(got.len(), 8);
                assert!((got[0] - 1.0).abs() < 1e-6);
                assert!((got[4] - 0.75).abs() < 1e-6);
                assert!((got[5] - 0.25).abs() < 1e-6);
            }
            other => panic!("bone_weights missing: {other:?}"),
        }
    }

    /// An unrigged mesh must not gain empty skinning arrays.
    #[test]
    fn import_gltf_without_skinning_omits_bone_arrays() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.glb");
        std::fs::write(&path, make_triangle_glb()).unwrap();
        let res = import_gltf(&path).unwrap();
        let sub = res.subresources.get("mesh_0_prim_0").expect("prim");
        assert!(sub.get_property("bone_indices").is_none());
        assert!(sub.get_property("bone_weights").is_none());
    }

    #[test]
    fn import_missing_file_errors() {
        let err = import_gltf(Path::new("/nonexistent/nope.glb")).unwrap_err();
        match err {
            EngineError::Io(_) => {}
            other => panic!("expected Io, got {other}"),
        }
    }

    #[test]
    fn unsupported_extensions_listed() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.glb");
        std::fs::write(&path, make_triangle_glb()).unwrap();
        let res = import_gltf(&path).unwrap();
        match res.get_property("unsupported_extensions") {
            Some(Variant::Array(a)) => {
                assert!(a
                    .to_vec()
                    .iter()
                    .any(|v| matches!(v, Variant::String(s) if s.contains("draco"))));
            }
            other => panic!("{other:?}"),
        }
    }

    /// glTF's CCW front face becomes Godot's CW one on import: the triangle
    /// authored `0, 1, 2` (counter-clockwise about its +Z normal) must come out
    /// `0, 2, 1`, exactly as Godot's `_parse_meshes` swaps it
    /// (`gltf_document.cpp:1486-1493`). Without the swap every imported model
    /// renders inside-out under the renderer's Godot-convention back-face cull.
    #[test]
    fn import_converts_ccw_triangles_to_godot_clockwise() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("t.glb");
        std::fs::write(&path, make_triangle_glb()).unwrap();
        let res = import_gltf(&path).unwrap();
        let sub = res.subresources.get("mesh_0_prim_0").expect("prim");
        let indices: Vec<i64> = match sub.get_property("indices") {
            Some(Variant::Array(a)) => a
                .to_vec()
                .iter()
                .map(|v| match v {
                    Variant::Int(i) => *i,
                    other => panic!("index {other:?}"),
                })
                .collect(),
            other => panic!("indices {other:?}"),
        };
        assert_eq!(indices, vec![0, 2, 1]);
    }

    #[test]
    fn import_fixture_quad_glb() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/quad.glb");
        let res = import_gltf(&path).expect("fixture quad.glb must import");
        assert_eq!(res.get_property("mesh_count"), Some(&Variant::Int(1)));
        let sub = res.subresources.get("mesh_0_prim_0").expect("prim");
        let verts = match sub.get_property("vertices") {
            Some(Variant::Array(a)) => a,
            _ => panic!("verts"),
        };
        assert_eq!(verts.len(), 4, "quad has 4 vertices");
        let indices = match sub.get_property("indices") {
            Some(Variant::Array(a)) => a,
            _ => panic!("indices"),
        };
        assert_eq!(indices.len(), 6, "two triangles");
    }

    #[test]
    fn parse_skins_and_animations_from_rigged_fixture() {
        use serde_json::json;

        // Binary blob: 2 inverse-bind MAT4 (128 B) + 2 time SCALARs (8 B) +
        // 2 translation VEC3 outputs (24 B) = 160 bytes.
        let ibm0: [f32; 16] = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let ibm1: [f32; 16] = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., -1., 0., 0., 1.,
        ];
        let mut bin = Vec::new();
        for m in [ibm0, ibm1] {
            for v in m {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        for t in [0.0f32, 1.0] {
            bin.extend_from_slice(&t.to_le_bytes());
        }
        for p in [[0.0f32, 0., 0.], [5., 0., 0.]] {
            for v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        assert_eq!(bin.len(), 160);
        let buffers = vec![bin];

        let doc = json!({
            "asset": {"version": "2.0"},
            "nodes": [
                {"name": "Armature", "children": [1]},
                {"name": "BoneA", "children": [2]},
                {"name": "BoneB"}
            ],
            "skins": [{"joints": [1, 2], "inverseBindMatrices": 0, "skeleton": 0}],
            "animations": [{
                "name": "Walk",
                "samplers": [{"input": 1, "output": 2, "interpolation": "LINEAR"}],
                "channels": [{"sampler": 0, "target": {"node": 1, "path": "translation"}}]
            }],
            "buffers": [{"byteLength": 160}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 128},
                {"buffer": 0, "byteOffset": 128, "byteLength": 8},
                {"buffer": 0, "byteOffset": 136, "byteLength": 24}
            ],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 2, "type": "MAT4"},
                {"bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR"},
                {"bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3"}
            ]
        });

        // Skin → bone hierarchy + bind poses.
        let skins = parse_skins(&doc, &buffers).expect("parse skins");
        assert_eq!(skins.len(), 1);
        let skin = &skins[0];
        assert_eq!(skin.joints, vec![1, 2]);
        // BoneA (joint 0) is a root within the skin; BoneB (joint 1) parents to it.
        assert_eq!(skin.parents, vec![None, Some(0)]);
        assert_eq!(skin.skeleton_root, Some(0));
        assert_eq!(skin.inverse_bind_matrices.len(), 2);
        assert_eq!(skin.inverse_bind_matrices[0][0], 1.0);
        assert_eq!(skin.inverse_bind_matrices[1][12], -1.0);

        // Animation → AnimationPlayer clip.
        let anims = parse_animations(&doc, &buffers).expect("parse animations");
        assert_eq!(anims.len(), 1);
        let clip = &anims[0];
        assert_eq!(clip.name, "Walk");
        assert_eq!(clip.length, 1.0);
        assert_eq!(clip.channels.len(), 1);
        let track = &clip.channels[0];
        assert_eq!(track.target_node, 1);
        assert_eq!(track.path, GltfAnimationPath::Translation);
        assert_eq!(track.interpolation, GltfInterpolation::Linear);
        assert_eq!(track.times, vec![0.0, 1.0]);
        assert_eq!(track.values, vec![0.0, 0.0, 0.0, 5.0, 0.0, 0.0]);
        assert_eq!(track.components, 3);
    }

    #[test]
    fn parse_skins_animations_absent_yields_empty() {
        use serde_json::json;
        let doc = json!({"asset": {"version": "2.0"}, "nodes": []});
        let buffers: Vec<Vec<u8>> = Vec::new();
        assert!(parse_skins(&doc, &buffers).unwrap().is_empty());
        assert!(parse_animations(&doc, &buffers).unwrap().is_empty());
    }
}
