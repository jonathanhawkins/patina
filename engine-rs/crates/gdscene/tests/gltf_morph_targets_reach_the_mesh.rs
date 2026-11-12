//! pat-s3rrv.23 item (b) — glTF morph targets reach `Mesh3D` and blend.
//!
//! `gdrender3d::morph` has carried the blend-shape maths and a full set of unit
//! tests since it landed, and had **zero consumers**: nothing in the engine
//! could produce a `MorphTarget`, so no file a user authored could reach it. Its
//! own module doc says as much — *"glTF morph import can populate `MorphTarget`s
//! as a follow-up"*. This is that follow-up, and it pins the whole chain:
//! `.gltf` → `import_gltf` → `mesh3d_from_gltf_resource` → `apply_morph`.
//!
//! Ground truth: `modules/gltf/gltf_document.cpp:1784-1833` for `targets`, with
//! names from the mesh-level `extras.targetNames` (`gltf_document.cpp:1425`) and
//! a `morph_<k>` fallback.
//!
//! ⚠️ The base triangle sits **away from the origin** and its deltas are
//! asymmetric — one axis per vertex, distinct magnitudes, one negative. A
//! fixture based at the origin cannot tell a delta from an absolute position,
//! because there they are the same number; a symmetric one cannot tell a correct
//! import from one that transposes or reorders components.

use gdcore::math::Vector3;
use gdrender3d::morph::{apply_morph, MorphTarget};
use gdscene::render_server_3d::mesh3d_from_gltf_resource;
use gdserver3d::mesh::Mesh3D;

/// Base triangle — no vertex at the origin, every component distinct.
const BASE: [[f32; 3]; 3] = [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]];
/// Target 0, named "Smile" in the file.
const SMILE: [[f32; 3]; 3] = [[0.5, 0.0, 0.0], [0.0, -1.5, 0.0], [0.0, 0.0, 2.0]];
/// Target 1, deliberately left unnamed so the `morph_<k>` fallback is exercised.
const OTHER: [[f32; 3]; 3] = [[10.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]];

/// Base64 without padding — every buffer here is a multiple of three bytes, the
/// same shape the importer's own `.gltf` fixtures use.
fn b64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    assert_eq!(bytes.len() % 3, 0, "fixture buffer must be a multiple of 3");
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | (c[2] as u32);
        s.push(T[((n >> 18) & 63) as usize] as char);
        s.push(T[((n >> 12) & 63) as usize] as char);
        s.push(T[((n >> 6) & 63) as usize] as char);
        s.push(T[(n & 63) as usize] as char);
    }
    s
}

fn le_bytes(groups: &[[f32; 3]]) -> Vec<u8> {
    let mut out = Vec::new();
    for g in groups {
        for v in g {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

fn v3(a: [f32; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

fn approx(a: Vector3, b: Vector3) -> bool {
    (a.x - b.x).abs() < 1e-5 && (a.y - b.y).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5
}

/// Writes a `.gltf` and runs it through the real import path.
///
/// `with_targets` toggles the morph targets; the no-targets case is the negative
/// control that proves shapes are read from the file rather than invented.
fn import(with_targets: bool) -> Mesh3D {
    let mut bin = le_bytes(&BASE);
    bin.extend(le_bytes(&SMILE));
    bin.extend(le_bytes(&OTHER));
    let encoded = b64(&bin);
    let len = bin.len();

    // ⚠️ `targetNames` names only the FIRST target. Naming both would hide the
    // `morph_<k>` fallback, which is the branch a real exporter hits — glTF
    // makes `extras.targetNames` optional and plenty of tools omit it.
    let targets = if with_targets {
        r#", "targets": [{"POSITION": 1}, {"POSITION": 2}]"#
    } else {
        ""
    };
    let extras = if with_targets {
        r#", "extras": {"targetNames": ["Smile"]}"#
    } else {
        ""
    };

    let json = format!(
        r#"{{
          "asset": {{"version": "2.0"}},
          "scenes": [{{"nodes": [0]}}],
          "nodes": [{{"name": "Root", "mesh": 0}}],
          "buffers": [{{"byteLength": {len}, "uri": "data:application/octet-stream;base64,{encoded}"}}],
          "bufferViews": [
            {{"buffer": 0, "byteOffset": 0,  "byteLength": 36}},
            {{"buffer": 0, "byteOffset": 36, "byteLength": 36}},
            {{"buffer": 0, "byteOffset": 72, "byteLength": 36}}
          ],
          "accessors": [
            {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3"}},
            {{"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}},
            {{"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3"}}
          ],
          "meshes": [{{"name": "Face", "primitives": [{{"attributes": {{"POSITION": 0}}{targets}}}]{extras}}}]
        }}"#
    );

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("face.gltf");
    std::fs::write(&path, json).unwrap();
    let resource = gdresource::import_gltf(&path).expect("gltf imports");
    mesh3d_from_gltf_resource(&resource).expect("resource converts to a Mesh3D")
}

/// **The acceptance test for item (b) layer 1+2**: the deltas survive the file,
/// the importer and the conversion, and drive the existing blend maths.
#[test]
fn gltf_morph_targets_reach_the_mesh() {
    let mesh = import(true);

    assert_eq!(
        mesh.blend_shapes.len(),
        2,
        "both `primitives[].targets` entries must arrive as blend shapes",
    );
    assert_eq!(
        mesh.blend_shapes[0].name, "Smile",
        "an authored `extras.targetNames` entry is the shape's name",
    );
    assert_eq!(
        mesh.blend_shapes[1].name, "morph_1",
        "a target the file does not name falls back to Godot's `morph_<k>`, \
         numbered by its position — `morph_0` here would mean the index was lost",
    );

    // ⚠️ Deltas, NOT absolute positions. Godot converts these to absolutes on
    // import because its ArrayMesh format stores absolutes; `apply_morph` adds
    // them to the base itself, so storing absolutes would apply the base twice.
    // (0.5, 0, 0) is the delta; (1.5, 2, 3) would be the absolute.
    let smile = &mesh.blend_shapes[0].vertex_deltas;
    assert_eq!(smile.len(), 3, "one delta per vertex");
    for (i, expected) in SMILE.iter().enumerate() {
        assert!(
            approx(smile[i], v3(*expected)),
            "delta {i} must be the raw accessor value {expected:?}, got {:?} — an \
             absolute position here means the base was folded in at import",
            smile[i],
        );
    }

    // The whole point: the existing, already-tested blend maths can now be fed.
    let targets: Vec<MorphTarget> = mesh
        .blend_shapes
        .iter()
        .map(|bs| MorphTarget::new(&bs.name, bs.vertex_deltas.clone()))
        .collect();
    // Full weight on "Smile" only, so target 1's 10.0 on x must NOT appear.
    let morphed = apply_morph(&mesh.vertices, &targets, &[1.0, 0.0]);

    let expected = [
        Vector3::new(1.5, 2.0, 3.0),
        Vector3::new(4.0, 3.5, 6.0),
        Vector3::new(7.0, 8.0, 11.0),
    ];
    for (i, want) in expected.iter().enumerate() {
        assert!(
            approx(morphed[i], *want),
            "vertex {i} at full Smile weight must be base + delta = {want:?}, got {:?}",
            morphed[i],
        );
    }
}

/// The negative control: a primitive with no `targets` yields no blend shapes.
///
/// Without this, an implementation that fabricated a shape per primitive — or
/// one that read the wrong accessor and happened to find data — would satisfy
/// every assertion above.
#[test]
fn a_primitive_without_targets_has_no_blend_shapes() {
    let mesh = import(false);
    assert!(
        mesh.blend_shapes.is_empty(),
        "a mesh with no morph targets must carry none, got {:?}",
        mesh.blend_shapes
            .iter()
            .map(|b| &b.name)
            .collect::<Vec<_>>(),
    );
    // …and the base geometry is unaffected by the morph plumbing.
    assert_eq!(mesh.vertices.len(), 3);
    assert!(approx(mesh.vertices[2], v3(BASE[2])));
}
