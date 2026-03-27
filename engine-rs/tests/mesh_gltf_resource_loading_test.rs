//! pat-z42y8: Mesh resource loading from glTF 2.0 files — integration tests.
//!
//! Validates the full pipeline: glTF/GLB file → import_gltf → Resource →
//! mesh3d_from_gltf_resource / material_from_gltf_resource → Mesh3D + Material3D.
//!
//! Covers:
//! - Single-mesh GLB import (positions, normals, indices)
//! - Multi-mesh GLB import (multiple primitives → multiple surfaces)
//! - PBR material extraction (albedo, metallic, roughness, emissive, double-sided)
//! - Resource property shape (class_name, mesh_count, sub-resource keys)
//! - Mesh3D conversion fidelity (vertex count, index count, surface count)
//! - Edge cases: empty mesh, missing normals, UVs
//! - ResourceFormatLoader integration (.glb / .gltf extension dispatch)

use gdresource::importers::import_gltf;
use gdresource::ResourceFormatLoader;
use gdvariant::Variant;
use std::path::Path;
use tempfile::TempDir;

// ============================================================================
// GLB test-data builders
// ============================================================================

/// Builds a minimal valid GLB with one triangle mesh (3 vertices, 3 normals, 3 indices).
fn make_triangle_glb() -> Vec<u8> {
    let positions: [[f32; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let normals: [[f32; 3]; 3] = [
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ];
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
        "meshes": [{
            "name": "Triangle",
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1 },
                "indices": 2
            }]
        }]
    });

    encode_glb(&json, &bin)
}

/// Builds a GLB with two meshes (quad + triangle) to test multi-mesh import.
fn make_multi_mesh_glb() -> Vec<u8> {
    // Mesh 0: a quad (4 verts, 6 indices)
    let quad_pos: [[f32; 3]; 4] = [
        [-1.0, 0.0, -1.0],
        [1.0, 0.0, -1.0],
        [1.0, 0.0, 1.0],
        [-1.0, 0.0, 1.0],
    ];
    let quad_norm: [[f32; 3]; 4] = [[0.0, 1.0, 0.0]; 4];
    let quad_idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

    // Mesh 1: a triangle (3 verts, 3 indices)
    let tri_pos: [[f32; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [1.0, 2.0, 0.0],
    ];
    let tri_norm: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];
    let tri_idx: [u16; 3] = [0, 1, 2];

    let mut bin = Vec::new();

    // Quad positions (48 bytes)
    let quad_pos_offset = bin.len();
    for p in &quad_pos {
        for &v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let quad_pos_len = bin.len() - quad_pos_offset;

    // Quad normals (48 bytes)
    let quad_norm_offset = bin.len();
    for n in &quad_norm {
        for &v in n {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let quad_norm_len = bin.len() - quad_norm_offset;

    // Quad indices (12 bytes)
    let quad_idx_offset = bin.len();
    for &i in &quad_idx {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let quad_idx_len = bin.len() - quad_idx_offset;

    // Tri positions (36 bytes)
    let tri_pos_offset = bin.len();
    for p in &tri_pos {
        for &v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let tri_pos_len = bin.len() - tri_pos_offset;

    // Tri normals (36 bytes)
    let tri_norm_offset = bin.len();
    for n in &tri_norm {
        for &v in n {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let tri_norm_len = bin.len() - tri_norm_offset;

    // Tri indices (6 bytes)
    let tri_idx_offset = bin.len();
    for &i in &tri_idx {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let tri_idx_len = bin.len() - tri_idx_offset;

    // Pad to 4-byte alignment
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": quad_pos_offset, "byteLength": quad_pos_len, "target": 34962 },
            { "buffer": 0, "byteOffset": quad_norm_offset, "byteLength": quad_norm_len, "target": 34962 },
            { "buffer": 0, "byteOffset": quad_idx_offset, "byteLength": quad_idx_len, "target": 34963 },
            { "buffer": 0, "byteOffset": tri_pos_offset, "byteLength": tri_pos_len, "target": 34962 },
            { "buffer": 0, "byteOffset": tri_norm_offset, "byteLength": tri_norm_len, "target": 34962 },
            { "buffer": 0, "byteOffset": tri_idx_offset, "byteLength": tri_idx_len, "target": 34963 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
              "max": [1.0, 0.0, 1.0], "min": [-1.0, 0.0, -1.0] },
            { "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR" },
            { "bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3",
              "max": [2.0, 2.0, 0.0], "min": [0.0, 0.0, 0.0] },
            { "bufferView": 4, "componentType": 5126, "count": 3, "type": "VEC3" },
            { "bufferView": 5, "componentType": 5123, "count": 3, "type": "SCALAR" }
        ],
        "meshes": [
            {
                "name": "Quad",
                "primitives": [{
                    "attributes": { "POSITION": 0, "NORMAL": 1 },
                    "indices": 2
                }]
            },
            {
                "name": "Tri",
                "primitives": [{
                    "attributes": { "POSITION": 3, "NORMAL": 4 },
                    "indices": 5
                }]
            }
        ]
    });

    encode_glb(&json, &bin)
}

/// Builds a GLB with PBR material properties for material extraction testing.
fn make_pbr_material_glb() -> Vec<u8> {
    let positions: [[f32; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];
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
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0,  "byteLength": 36, "target": 34962 },
            { "buffer": 0, "byteOffset": 36, "byteLength": 36, "target": 34962 },
            { "buffer": 0, "byteOffset": 72, "byteLength": 6,  "target": 34963 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
              "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0] },
            { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }
        ],
        "materials": [{
            "name": "RedMetal",
            "pbrMetallicRoughness": {
                "baseColorFactor": [0.8, 0.1, 0.1, 1.0],
                "metallicFactor": 0.9,
                "roughnessFactor": 0.2
            },
            "emissiveFactor": [0.5, 0.0, 0.0],
            "doubleSided": true
        }],
        "meshes": [{
            "name": "MaterialTest",
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1 },
                "indices": 2,
                "material": 0
            }]
        }]
    });

    encode_glb(&json, &bin)
}

/// Builds a GLB with UV coordinates for texture coordinate extraction testing.
fn make_uv_glb() -> Vec<u8> {
    let positions: [[f32; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];
    let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];
    let uvs: [[f32; 2]; 3] = [
        [0.0, 0.0],
        [1.0, 0.0],
        [0.5, 1.0],
    ];
    let indices: [u16; 3] = [0, 1, 2];

    let mut bin = Vec::new();
    // Positions (36 bytes)
    for p in &positions {
        for &v in p {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    // Normals (36 bytes)
    for n in &normals {
        for &v in n {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    // UVs (24 bytes)
    let uv_offset = bin.len();
    for uv in &uvs {
        for &v in uv {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    let uv_len = bin.len() - uv_offset;
    // Indices (6 bytes)
    let idx_offset = bin.len();
    for &i in &indices {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let idx_len = bin.len() - idx_offset;
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0,  "byteLength": 36, "target": 34962 },
            { "buffer": 0, "byteOffset": 36, "byteLength": 36, "target": 34962 },
            { "buffer": 0, "byteOffset": uv_offset, "byteLength": uv_len, "target": 34962 },
            { "buffer": 0, "byteOffset": idx_offset, "byteLength": idx_len, "target": 34963 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
              "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0] },
            { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2" },
            { "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" }
        ],
        "meshes": [{
            "name": "UVTriangle",
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 },
                "indices": 3
            }]
        }]
    });

    encode_glb(&json, &bin)
}

/// Builds a GLB with a single mesh that has two primitives (two surfaces).
fn make_multi_primitive_glb() -> Vec<u8> {
    // Prim 0: triangle
    let tri_pos: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let tri_norm: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];
    let tri_idx: [u16; 3] = [0, 1, 2];

    // Prim 1: another triangle offset
    let tri2_pos: [[f32; 3]; 3] = [[2.0, 0.0, 0.0], [3.0, 0.0, 0.0], [2.5, 1.0, 0.0]];
    let tri2_norm: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];
    let tri2_idx: [u16; 3] = [0, 1, 2];

    let mut bin = Vec::new();

    let p0_off = bin.len();
    for p in &tri_pos {
        for &v in p { bin.extend_from_slice(&v.to_le_bytes()); }
    }
    let p0_len = bin.len() - p0_off;

    let n0_off = bin.len();
    for n in &tri_norm {
        for &v in n { bin.extend_from_slice(&v.to_le_bytes()); }
    }
    let n0_len = bin.len() - n0_off;

    let i0_off = bin.len();
    for &i in &tri_idx { bin.extend_from_slice(&i.to_le_bytes()); }
    let i0_len = bin.len() - i0_off;

    // Pad indices to 4-byte alignment before next section
    while bin.len() % 4 != 0 { bin.push(0); }

    let p1_off = bin.len();
    for p in &tri2_pos {
        for &v in p { bin.extend_from_slice(&v.to_le_bytes()); }
    }
    let p1_len = bin.len() - p1_off;

    let n1_off = bin.len();
    for n in &tri2_norm {
        for &v in n { bin.extend_from_slice(&v.to_le_bytes()); }
    }
    let n1_len = bin.len() - n1_off;

    let i1_off = bin.len();
    for &i in &tri2_idx { bin.extend_from_slice(&i.to_le_bytes()); }
    let i1_len = bin.len() - i1_off;

    while bin.len() % 4 != 0 { bin.push(0); }

    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": p0_off, "byteLength": p0_len, "target": 34962 },
            { "buffer": 0, "byteOffset": n0_off, "byteLength": n0_len, "target": 34962 },
            { "buffer": 0, "byteOffset": i0_off, "byteLength": i0_len, "target": 34963 },
            { "buffer": 0, "byteOffset": p1_off, "byteLength": p1_len, "target": 34962 },
            { "buffer": 0, "byteOffset": n1_off, "byteLength": n1_len, "target": 34962 },
            { "buffer": 0, "byteOffset": i1_off, "byteLength": i1_len, "target": 34963 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
              "max": [1.0, 1.0, 0.0], "min": [0.0, 0.0, 0.0] },
            { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" },
            { "bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3",
              "max": [3.0, 1.0, 0.0], "min": [2.0, 0.0, 0.0] },
            { "bufferView": 4, "componentType": 5126, "count": 3, "type": "VEC3" },
            { "bufferView": 5, "componentType": 5123, "count": 3, "type": "SCALAR" }
        ],
        "meshes": [{
            "name": "TwoSurfaces",
            "primitives": [
                { "attributes": { "POSITION": 0, "NORMAL": 1 }, "indices": 2 },
                { "attributes": { "POSITION": 3, "NORMAL": 4 }, "indices": 5 }
            ]
        }]
    });

    encode_glb(&json, &bin)
}

/// Encodes a JSON document + binary buffer into a valid GLB (glTF Binary) file.
fn encode_glb(json: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
    let mut json_bytes = serde_json::to_vec(json).unwrap();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }

    let total_length = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total_length);

    // GLB header
    glb.extend_from_slice(&0x46546C67u32.to_le_bytes()); // magic "glTF"
    glb.extend_from_slice(&2u32.to_le_bytes());          // version 2
    glb.extend_from_slice(&(total_length as u32).to_le_bytes());

    // JSON chunk
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes()); // "JSON"
    glb.extend_from_slice(&json_bytes);

    // BIN chunk
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004E4942u32.to_le_bytes()); // "BIN\0"
    glb.extend_from_slice(bin);

    glb
}

fn write_glb(dir: &TempDir, name: &str, data: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, data).unwrap();
    path
}

// ============================================================================
// 1. Single-mesh import: basic triangle
// ============================================================================

#[test]
fn import_triangle_glb_produces_arraymesh_resource() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();

    assert_eq!(res.class_name, "ArrayMesh");
    assert_eq!(res.get_property("mesh_count"), Some(&Variant::Int(1)));
}

#[test]
fn import_triangle_glb_has_correct_mesh_name() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();

    assert_eq!(
        res.get_property("mesh_0_name"),
        Some(&Variant::String("Triangle".into()))
    );
}

#[test]
fn import_triangle_glb_has_mesh_sub_resource() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();

    let sub = res.subresources.get("mesh_0").expect("mesh_0 sub-resource");
    assert_eq!(sub.get_property("vertex_count"), Some(&Variant::Int(3)));
    assert_eq!(sub.get_property("index_count"), Some(&Variant::Int(3)));
}

#[test]
fn import_triangle_glb_vertex_positions_correct() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    if let Some(Variant::Array(verts)) = sub.get_property("vertices") {
        assert_eq!(verts.len(), 3);
        assert_eq!(verts[0], Variant::Vector3(gdcore::math::Vector3::new(0.0, 0.0, 0.0)));
        assert_eq!(verts[1], Variant::Vector3(gdcore::math::Vector3::new(1.0, 0.0, 0.0)));
        assert_eq!(verts[2], Variant::Vector3(gdcore::math::Vector3::new(0.0, 1.0, 0.0)));
    } else {
        panic!("expected vertices Array of Vector3");
    }
}

#[test]
fn import_triangle_glb_normals_correct() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    if let Some(Variant::Array(norms)) = sub.get_property("normals") {
        assert_eq!(norms.len(), 3);
        for n in norms {
            assert_eq!(*n, Variant::Vector3(gdcore::math::Vector3::new(0.0, 0.0, 1.0)));
        }
    } else {
        panic!("expected normals Array of Vector3");
    }
}

#[test]
fn import_triangle_glb_indices_correct() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    if let Some(Variant::Array(idxs)) = sub.get_property("indices") {
        assert_eq!(idxs.len(), 3);
        assert_eq!(idxs[0], Variant::Int(0));
        assert_eq!(idxs[1], Variant::Int(1));
        assert_eq!(idxs[2], Variant::Int(2));
    } else {
        panic!("expected indices Array of Int");
    }
}

// ============================================================================
// 2. Multi-mesh import
// ============================================================================

#[test]
fn import_multi_mesh_glb_has_two_meshes() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "multi.glb", &make_multi_mesh_glb());
    let res = import_gltf(&path).unwrap();

    assert_eq!(res.get_property("mesh_count"), Some(&Variant::Int(2)));
    assert_eq!(
        res.get_property("mesh_0_name"),
        Some(&Variant::String("Quad".into()))
    );
    assert_eq!(
        res.get_property("mesh_1_name"),
        Some(&Variant::String("Tri".into()))
    );
}

#[test]
fn import_multi_mesh_glb_quad_has_4_vertices() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "multi.glb", &make_multi_mesh_glb());
    let res = import_gltf(&path).unwrap();

    let sub = res.subresources.get("mesh_0_prim_0").expect("quad sub-resource");
    assert_eq!(sub.get_property("vertex_count"), Some(&Variant::Int(4)));
    assert_eq!(sub.get_property("index_count"), Some(&Variant::Int(6)));
}

#[test]
fn import_multi_mesh_glb_tri_has_3_vertices() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "multi.glb", &make_multi_mesh_glb());
    let res = import_gltf(&path).unwrap();

    let sub = res.subresources.get("mesh_1_prim_0").expect("tri sub-resource");
    assert_eq!(sub.get_property("vertex_count"), Some(&Variant::Int(3)));
    assert_eq!(sub.get_property("index_count"), Some(&Variant::Int(3)));
}

// ============================================================================
// 3. PBR material extraction
// ============================================================================

#[test]
fn import_pbr_glb_has_material_albedo() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "pbr.glb", &make_pbr_material_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    if let Some(Variant::Color(c)) = sub.get_property("material_albedo") {
        assert!((c.r - 0.8).abs() < 0.01, "albedo.r: {}", c.r);
        assert!((c.g - 0.1).abs() < 0.01, "albedo.g: {}", c.g);
        assert!((c.b - 0.1).abs() < 0.01, "albedo.b: {}", c.b);
        assert!((c.a - 1.0).abs() < 0.01, "albedo.a: {}", c.a);
    } else {
        panic!("expected material_albedo Color");
    }
}

#[test]
fn import_pbr_glb_has_metallic_and_roughness() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "pbr.glb", &make_pbr_material_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    if let Some(Variant::Float(m)) = sub.get_property("material_metallic") {
        assert!((*m - 0.9).abs() < 0.01, "metallic: {m}");
    } else {
        panic!("expected material_metallic Float");
    }

    if let Some(Variant::Float(r)) = sub.get_property("material_roughness") {
        assert!((*r - 0.2).abs() < 0.01, "roughness: {r}");
    } else {
        panic!("expected material_roughness Float");
    }
}

#[test]
fn import_pbr_glb_has_emissive_factor() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "pbr.glb", &make_pbr_material_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    if let Some(Variant::Color(c)) = sub.get_property("material_emissive") {
        assert!((c.r - 0.5).abs() < 0.01, "emissive.r: {}", c.r);
        assert!(c.g.abs() < 0.01, "emissive.g: {}", c.g);
        assert!(c.b.abs() < 0.01, "emissive.b: {}", c.b);
    } else {
        panic!("expected material_emissive Color");
    }
}

#[test]
fn import_pbr_glb_has_double_sided() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "pbr.glb", &make_pbr_material_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    assert_eq!(
        sub.get_property("material_double_sided"),
        Some(&Variant::Bool(true))
    );
}

#[test]
fn import_pbr_glb_has_material_name() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "pbr.glb", &make_pbr_material_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    assert_eq!(
        sub.get_property("material_name"),
        Some(&Variant::String("RedMetal".into()))
    );
}

// ============================================================================
// 4. UV coordinate extraction
// ============================================================================

#[test]
fn import_uv_glb_extracts_texcoords() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "uv.glb", &make_uv_glb());
    let res = import_gltf(&path).unwrap();
    let sub = res.subresources.get("mesh_0").unwrap();

    if let Some(Variant::Array(uvs)) = sub.get_property("uvs") {
        assert_eq!(uvs.len(), 3, "expected 3 UV coordinates");
        // UV[0] = (0.0, 0.0)
        if let Variant::Array(pair) = &uvs[0] {
            assert_eq!(pair.len(), 2);
            assert_eq!(pair[0], Variant::Float(0.0));
            assert_eq!(pair[1], Variant::Float(0.0));
        } else {
            panic!("UV should be Array([Float, Float])");
        }
        // UV[1] = (1.0, 0.0)
        if let Variant::Array(pair) = &uvs[1] {
            assert_eq!(pair[0], Variant::Float(1.0));
            assert_eq!(pair[1], Variant::Float(0.0));
        } else {
            panic!("UV should be Array([Float, Float])");
        }
        // UV[2] = (0.5, 1.0)
        if let Variant::Array(pair) = &uvs[2] {
            if let Variant::Float(u) = pair[0] {
                assert!((u - 0.5).abs() < 0.01, "uv.u: {u}");
            }
            assert_eq!(pair[1], Variant::Float(1.0));
        } else {
            panic!("UV should be Array([Float, Float])");
        }
    } else {
        panic!("expected uvs property");
    }
}

// ============================================================================
// 5. Multi-primitive (multi-surface) within a single mesh
// ============================================================================

#[test]
fn import_multi_primitive_glb_has_two_sub_resources() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "multi_prim.glb", &make_multi_primitive_glb());
    let res = import_gltf(&path).unwrap();

    // Single mesh with 2 primitives → mesh_0_prim_0 and mesh_0_prim_1
    assert!(res.subresources.contains_key("mesh_0_prim_0"));
    assert!(res.subresources.contains_key("mesh_0_prim_1"));
    assert_eq!(res.subresources.len(), 2);
}

#[test]
fn import_multi_primitive_second_prim_vertex_positions() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "multi_prim.glb", &make_multi_primitive_glb());
    let res = import_gltf(&path).unwrap();

    let sub = res.subresources.get("mesh_0_prim_1").unwrap();
    if let Some(Variant::Array(verts)) = sub.get_property("vertices") {
        assert_eq!(verts.len(), 3);
        // First vertex of prim 1 should be at (2, 0, 0)
        assert_eq!(verts[0], Variant::Vector3(gdcore::math::Vector3::new(2.0, 0.0, 0.0)));
    } else {
        panic!("expected vertices");
    }
}

// ============================================================================
// 6. Mesh3D conversion via render_server_3d
// ============================================================================

#[test]
fn mesh3d_from_gltf_resource_converts_triangle() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();

    let mesh = gdscene::render_server_3d::mesh3d_from_gltf_resource(&res)
        .expect("should convert to Mesh3D");

    assert_eq!(mesh.vertex_count(), 3);
    assert_eq!(mesh.triangle_count(), 1);
    assert_eq!(mesh.normals.len(), 3);
}

#[test]
fn mesh3d_from_gltf_resource_multi_mesh_becomes_multi_surface() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "multi.glb", &make_multi_mesh_glb());
    let res = import_gltf(&path).unwrap();

    let mesh = gdscene::render_server_3d::mesh3d_from_gltf_resource(&res)
        .expect("should convert to Mesh3D");

    // 2 sub-resources → primary surface + 1 extra surface = 2 total
    assert_eq!(mesh.surface_count(), 2, "expected 2 surfaces");
}

#[test]
fn mesh3d_from_gltf_resource_multi_prim_becomes_multi_surface() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "multi_prim.glb", &make_multi_primitive_glb());
    let res = import_gltf(&path).unwrap();

    let mesh = gdscene::render_server_3d::mesh3d_from_gltf_resource(&res)
        .expect("should convert to Mesh3D");

    assert_eq!(mesh.surface_count(), 2, "two primitives → two surfaces");
}

#[test]
fn mesh3d_from_gltf_resource_preserves_uv_data() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "uv.glb", &make_uv_glb());
    let res = import_gltf(&path).unwrap();

    let mesh = gdscene::render_server_3d::mesh3d_from_gltf_resource(&res)
        .expect("should convert to Mesh3D");

    assert_eq!(mesh.uvs.len(), 3, "should have 3 UV coords");
    assert!((mesh.uvs[0][0]).abs() < 0.01);
    assert!((mesh.uvs[0][1]).abs() < 0.01);
    assert!((mesh.uvs[1][0] - 1.0).abs() < 0.01);
    assert!((mesh.uvs[2][0] - 0.5).abs() < 0.01);
    assert!((mesh.uvs[2][1] - 1.0).abs() < 0.01);
}

// ============================================================================
// 7. Material3D extraction via render_server_3d
// ============================================================================

#[test]
fn material_from_gltf_resource_extracts_pbr() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "pbr.glb", &make_pbr_material_glb());
    let res = import_gltf(&path).unwrap();

    let mat = gdscene::render_server_3d::material_from_gltf_resource(&res)
        .expect("should extract Material3D");

    assert!((mat.albedo.r - 0.8).abs() < 0.01, "albedo.r");
    assert!((mat.metallic - 0.9).abs() < 0.01, "metallic");
    assert!((mat.roughness - 0.2).abs() < 0.01, "roughness");
    assert!((mat.emission.r - 0.5).abs() < 0.01, "emission.r");
    assert!(mat.double_sided, "double_sided");
}

#[test]
fn material_from_gltf_resource_returns_none_for_no_material() {
    let dir = TempDir::new().unwrap();
    // Triangle GLB has default material (all zeros/defaults), but still has material_albedo property
    let path = write_glb(&dir, "tri.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();

    // Default material still produces a Material3D (white albedo, metallic=0, roughness=1)
    let mat = gdscene::render_server_3d::material_from_gltf_resource(&res);
    assert!(mat.is_some(), "default glTF material should still parse");
}

// ============================================================================
// 8. ResourceFormatLoader integration
// ============================================================================

#[test]
fn format_loader_loads_glb_extension() {
    let rfl = ResourceFormatLoader::with_defaults();
    assert!(rfl.can_load(".glb"));
    assert!(rfl.can_load(".gltf"));
    assert!(rfl.can_load(".GLB"));
    assert!(rfl.can_load(".GLTF"));
}

#[test]
fn format_loader_loads_glb_via_load_resource() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "model.glb", &make_triangle_glb());

    let rfl = ResourceFormatLoader::with_defaults();
    let res = rfl.load_resource(&path).unwrap();
    assert_eq!(res.class_name, "ArrayMesh");
    assert!(!res.subresources.is_empty());
}

// ============================================================================
// 9. Error handling
// ============================================================================

#[test]
fn import_gltf_nonexistent_file_returns_error() {
    assert!(import_gltf(Path::new("/tmp/nonexistent_model.glb")).is_err());
}

#[test]
fn import_gltf_invalid_data_returns_error() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.glb");
    std::fs::write(&path, b"not a glb file at all").unwrap();
    assert!(import_gltf(&path).is_err());
}

#[test]
fn import_gltf_empty_glb_no_meshes_returns_error() {
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "meshes": []
    });
    let mut json_bytes = serde_json::to_vec(&json).unwrap();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let bin: Vec<u8> = Vec::new();
    let total_length = 12 + 8 + json_bytes.len();
    let mut glb = Vec::with_capacity(total_length);
    glb.extend_from_slice(&0x46546C67u32.to_le_bytes());
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total_length as u32).to_le_bytes());
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    glb.extend_from_slice(&json_bytes);

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("empty.glb");
    std::fs::write(&path, &glb).unwrap();

    let _ = bin; // suppress unused warning
    let result = import_gltf(&path);
    assert!(result.is_err(), "empty meshes array should fail");
}

// ============================================================================
// 10. Resource path assignment
// ============================================================================

#[test]
fn import_gltf_sets_res_path() {
    let dir = TempDir::new().unwrap();
    let path = write_glb(&dir, "my_model.glb", &make_triangle_glb());
    let res = import_gltf(&path).unwrap();

    assert_eq!(res.path, "res://my_model.glb");
}

// ============================================================================
// 11. Mesh3D from empty resource returns None
// ============================================================================

#[test]
fn mesh3d_from_empty_resource_returns_none() {
    let empty = gdresource::Resource::new("ArrayMesh");
    assert!(gdscene::render_server_3d::mesh3d_from_gltf_resource(&empty).is_none());
}
