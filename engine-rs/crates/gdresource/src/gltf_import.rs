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
//! - Materials: `pbrMetallicRoughness` baseColor / metallic / roughness + emissive + doubleSided
//! - Nodes: name, children, mesh index, TRS or 4×4 matrix
//!
//! # Unsupported (documented)
//! - Morph targets, sparse accessors, Draco/meshopt compression
//! - Skinned mesh skinning matrices / animation playback
//! - Image / texture sampling (albedo texture URI is recorded but not decoded)
//! - Cameras, lights, extensions beyond core 2.0
//! - Multiple scenes (only default / first scene is imported)

use std::collections::HashMap;
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
                json_text = Some(s.trim_end_matches(|c: char| c == '\0' || c == ' ').to_string());
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

    let mut doc = Resource::new("GltfDocument");
    doc.path = path
        .to_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.display().to_string());

    // --- materials ---
    let materials = json
        .get("materials")
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
                        Variant::Array(vec![
                            Variant::Float(uv[0] as f64),
                            Variant::Float(uv[1] as f64),
                        ])
                    })
                    .collect();
                sub.set_property("uvs", Variant::Array(uv_variants));
            }

            // indices (optional — if missing, sequential triangle list)
            if let Some(i_acc) = prim.get("indices").and_then(|v| v.as_u64()) {
                let indices = read_index_accessor(json, &buffers, i_acc as usize)?;
                sub.set_property(
                    "indices",
                    Variant::Array(indices.iter().map(|&i| Variant::Int(i as i64)).collect()),
                );
            } else {
                let n = positions.len() as i64;
                let indices: Vec<Variant> = (0..n).map(Variant::Int).collect();
                sub.set_property("indices", Variant::Array(indices));
            }

            // material
            if let Some(mat_idx) = prim.get("material").and_then(|v| v.as_u64()) {
                let mi_u = mat_idx as usize;
                if let Some(mat) = materials.get(mi_u) {
                    apply_material_props(&mut sub, mat);
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
        let mut d = HashMap::new();
        d.insert("index".into(), Variant::Int(ni as i64));
        let name = node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Node")
            .to_string();
        d.insert("name".into(), Variant::String(name));
        if let Some(mesh) = node.get("mesh").and_then(|v| v.as_u64()) {
            d.insert("mesh".into(), Variant::Int(mesh as i64));
        }
        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            let kids: Vec<Variant> = children
                .iter()
                .filter_map(|c| c.as_u64().map(|u| Variant::Int(u as i64)))
                .collect();
            d.insert("children".into(), Variant::Array(kids));
        }
        let (t, r, s) = node_trs(node);
        d.insert(
            "translation".into(),
            Variant::Array(vec![
                Variant::Float(t[0] as f64),
                Variant::Float(t[1] as f64),
                Variant::Float(t[2] as f64),
            ]),
        );
        d.insert(
            "rotation".into(),
            Variant::Array(vec![
                Variant::Float(r[0] as f64),
                Variant::Float(r[1] as f64),
                Variant::Float(r[2] as f64),
                Variant::Float(r[3] as f64),
            ]),
        );
        d.insert(
            "scale".into(),
            Variant::Array(vec![
                Variant::Float(s[0] as f64),
                Variant::Float(s[1] as f64),
                Variant::Float(s[2] as f64),
            ]),
        );
        node_dicts.push(Variant::Dictionary(d));
    }
    doc.set_property("nodes", Variant::Array(node_dicts));
    doc.set_property("node_count", Variant::Int(nodes.len() as i64));
    doc.set_property("mesh_count", Variant::Int(mesh_count));
    doc.set_property("primitive_count", Variant::Int(prim_total));
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
                doc.set_property("scene_roots", Variant::Array(root_ids));
            }
        }
    }

    Ok(Arc::new(doc))
}

/// Extensions we intentionally do not implement yet.
const UNSUPPORTED_EXTENSIONS: &[&str] = &[
    "KHR_draco_mesh_compression",
    "EXT_meshopt_compression",
    "KHR_materials_transmission",
    "KHR_materials_volume",
    "KHR_lights_punctual",
    "KHR_animation_pointer",
];

fn apply_material_props(sub: &mut Resource, mat: &Value) {
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

    if let Some(name) = mat.get("name").and_then(|v| v.as_str()) {
        sub.set_property("material_name", Variant::String(name.into()));
    }
}

fn node_trs(node: &Value) -> ([f32; 3], [f32; 4], [f32; 3]) {
    // matrix takes precedence over TRS if present (glTF spec)
    if let Some(m) = node.get("matrix").and_then(|v| v.as_array()) {
        if m.len() >= 16 {
            let f = |i: usize| m[i].as_f64().unwrap_or(0.0) as f32;
            // column-major 4x4: translation is m[12], m[13], m[14]
            let t = [f(12), f(13), f(14)];
            // crude scale from column lengths; rotation left as identity for matrix path
            let sx = (f(0) * f(0) + f(1) * f(1) + f(2) * f(2)).sqrt();
            let sy = (f(4) * f(4) + f(5) * f(5) + f(6) * f(6)).sqrt();
            let sz = (f(8) * f(8) + f(9) * f(9) + f(10) * f(10)).sqrt();
            return (t, [0.0, 0.0, 0.0, 1.0], [sx.max(1e-8), sy.max(1e-8), sz.max(1e-8)]);
        }
    }
    let t = read_f32_3(node.get("translation"), [0.0, 0.0, 0.0]);
    let r = read_f32_4(node.get("rotation"), [0.0, 0.0, 0.0, 1.0]);
    let s = read_f32_3(node.get("scale"), [1.0, 1.0, 1.0]);
    (t, r, s)
}

fn read_f32_3(v: Option<&Value>, default: [f32; 3]) -> [f32; 3] {
    let Some(arr) = v.and_then(|x| x.as_array()) else {
        return default;
    };
    [
        arr.first().and_then(|x| x.as_f64()).unwrap_or(default[0] as f64) as f32,
        arr.get(1).and_then(|x| x.as_f64()).unwrap_or(default[1] as f64) as f32,
        arr.get(2).and_then(|x| x.as_f64()).unwrap_or(default[2] as f64) as f32,
    ]
}

fn read_f32_4(v: Option<&Value>, default: [f32; 4]) -> [f32; 4] {
    let Some(arr) = v.and_then(|x| x.as_array()) else {
        return default;
    };
    [
        arr.first().and_then(|x| x.as_f64()).unwrap_or(default[0] as f64) as f32,
        arr.get(1).and_then(|x| x.as_f64()).unwrap_or(default[1] as f64) as f32,
        arr.get(2).and_then(|x| x.as_f64()).unwrap_or(default[2] as f64) as f32,
        arr.get(3).and_then(|x| x.as_f64()).unwrap_or(default[3] as f64) as f32,
    ]
}

// ---------------------------------------------------------------------------
// Buffers / accessors
// ---------------------------------------------------------------------------

fn load_buffers(
    json: &Value,
    path: &Path,
    glb_bin: Option<&[u8]>,
) -> EngineResult<Vec<Vec<u8>>> {
    let Some(bufs) = json.get("buffers").and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(bufs.len());
    for (i, buf) in bufs.iter().enumerate() {
        let byte_length = buf
            .get("byteLength")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
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
            return Err(EngineError::Parse(format!(
                "gltf: buffer {i} missing uri"
            )));
        }
    }
    Ok(out)
}

fn accessor_meta(json: &Value, accessor_idx: usize) -> EngineResult<AccessorMeta> {
    let accessors = json
        .get("accessors")
        .and_then(|v| v.as_array())
        .ok_or_else(|| EngineError::Parse("gltf: missing accessors".into()))?;
    let acc = accessors.get(accessor_idx).ok_or_else(|| {
        EngineError::Parse(format!("gltf: accessor {accessor_idx} out of range"))
    })?;
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
    let byte_offset = acc
        .get("byteOffset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

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
    let buffer_idx = view
        .get("buffer")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let view_offset = view
        .get("byteOffset")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let view_length = view
        .get("byteLength")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
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
    let buf = buffers.get(meta.buffer_idx).ok_or_else(|| {
        EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx))
    })?;
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
    let buf = buffers.get(meta.buffer_idx).ok_or_else(|| {
        EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx))
    })?;
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

fn read_index_accessor(
    json: &Value,
    buffers: &[Vec<u8>],
    accessor_idx: usize,
) -> EngineResult<Vec<u32>> {
    let meta = accessor_meta(json, accessor_idx)?;
    let comps = type_components(&meta.type_str);
    if comps != 1 {
        return Err(EngineError::Parse(
            "gltf: indices must be SCALAR".into(),
        ));
    }
    let cs = component_size(meta.component_type)?;
    let buf = buffers.get(meta.buffer_idx).ok_or_else(|| {
        EngineError::Parse(format!("gltf: buffer {} missing", meta.buffer_idx))
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use gdcore::math::Vector3;

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
        assert_eq!(res.class_name, "GltfDocument");
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
        assert_eq!(verts[0], Variant::Vector3(Vector3::new(0.0, 0.0, 0.0)));
        assert_eq!(verts[1], Variant::Vector3(Vector3::new(1.0, 0.0, 0.0)));
        assert_eq!(verts[2], Variant::Vector3(Vector3::new(0.0, 1.0, 0.0)));

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
        let node0 = match &nodes[0] {
            Variant::Dictionary(d) => d,
            _ => panic!("node dict"),
        };
        assert_eq!(node0.get("name"), Some(&Variant::String("TriNode".into())));
        assert_eq!(node0.get("mesh"), Some(&Variant::Int(0)));
        match node0.get("translation") {
            Some(Variant::Array(t)) => {
                assert_eq!(t[0], Variant::Float(1.0));
                assert_eq!(t[1], Variant::Float(2.0));
                assert_eq!(t[2], Variant::Float(3.0));
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
            const T: &[u8] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
        assert_eq!(verts[1], Variant::Vector3(Vector3::new(2.0, 0.0, 0.0)));
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
                assert!(a.iter().any(|v| matches!(v, Variant::String(s) if s.contains("draco"))));
            }
            other => panic!("{other:?}"),
        }
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
}
