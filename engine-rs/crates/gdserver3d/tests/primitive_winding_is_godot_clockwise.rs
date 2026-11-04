//! Every generated closed/one-sided primitive is wound with Godot's front-face
//! convention: CLOCKWISE seen from the side its normal points to.
//!
//! Godot ground truth: "Godot uses clockwise winding order for front faces of
//! triangle primitive modes" (`doc/classes/ArrayMesh.xml:49`), which is the
//! order `PlaneMesh::_create_mesh_array` and its siblings in
//! `scene/resources/3d/primitive_meshes.cpp` emit.
//!
//! ⚠️ Why this exists: the renderer culled with the OPPOSITE convention
//! (counter-clockwise front), pinned against `Mesh3D::cube`, which was itself
//! wound CCW on its X/Z faces and CW on its Y faces. So every upstream-ported
//! primitive was inside-out — a `PlaneMesh` floor seen from above was culled
//! ENTIRELY, which blanked every editor/Play render test standing on one — and
//! a box drew its bottom face where its top should be. A mesh that mixes the
//! two windings cannot be right under ANY cull convention, so this counts both
//! directions per mesh rather than trusting a sample triangle.

use gdcore::math::Vector3;
use gdserver3d::mesh::Mesh3D;

/// `(clockwise, counter_clockwise)` triangle counts, judged against the
/// vertex normals. Degenerate triangles (pole fans) count as neither.
fn winding(mesh: &Mesh3D) -> (usize, usize) {
    let (mut cw, mut ccw) = (0, 0);
    for t in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[t[k] as usize]);
        let geo = (b - a).cross(c - a);
        // Area relative to the triangle's size, so a sliver at a sphere pole
        // (float-noise area) is not read as a real face.
        let scale = (b - a).length() * (c - a).length();
        if scale <= 0.0 || geo.length() <= 1e-4 * scale {
            continue;
        }
        let n = [0, 1, 2]
            .map(|k| mesh.normals[t[k] as usize])
            .into_iter()
            .fold(Vector3::ZERO, |acc, v| acc + v);
        if geo.dot(n) < 0.0 {
            cw += 1;
        } else {
            ccw += 1;
        }
    }
    (cw, ccw)
}

fn assert_godot_clockwise(name: &str, mesh: &Mesh3D) {
    let (cw, ccw) = winding(mesh);
    assert!(cw > 0, "{name}: no non-degenerate triangles to judge");
    assert_eq!(
        ccw,
        0,
        "{name}: {ccw} of {} triangles are counter-clockwise seen from outside; \
         Godot's front face is clockwise (ArrayMesh.xml:49), so those faces are \
         culled from the side they face",
        cw + ccw
    );
}

#[test]
fn every_primitive_mesh_is_wound_clockwise_from_outside() {
    for class in [
        "BoxMesh",
        "SphereMesh",
        "CapsuleMesh",
        "CylinderMesh",
        "PlaneMesh",
        "QuadMesh",
        "PrismMesh",
        "TorusMesh",
        "TubeTrailMesh",
    ] {
        let mesh = gdserver3d::primitive_mesh::build_primitive(class, &|_| None)
            .unwrap_or_else(|| panic!("{class} builds"));
        assert_godot_clockwise(class, &mesh);
    }
}

/// The two hand-built engine meshes the editor uses for markers and defaults.
/// `Mesh3D::cube` was the half-inside-out one.
#[test]
fn engine_built_meshes_are_wound_clockwise_from_outside() {
    assert_godot_clockwise("Mesh3D::cube", &Mesh3D::cube(1.0));
    assert_godot_clockwise("Mesh3D::plane", &Mesh3D::plane(2.0));
    assert_godot_clockwise("Mesh3D::sphere", &Mesh3D::sphere(1.0, 8));
}

/// The CSG box has no normals of its own, so judge it against "away from the
/// centre" — it is convex and centred on the origin.
#[test]
fn csg_box_is_wound_clockwise_from_outside() {
    let mesh = gdserver3d::csg::CSGBox3D::default().to_mesh();
    for t in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[t[k] as usize]);
        let centre = (a + b + c) * (1.0 / 3.0);
        assert!(
            (b - a).cross(c - a).dot(centre) < 0.0,
            "CSG box triangle {a:?} {b:?} {c:?} is counter-clockwise from outside"
        );
    }
}

/// OBJ is counter-clockwise; Godot's importer reverses it
/// (`resource_importer_obj.cpp:336-338`). The flat normals must still point
/// out — they are computed from the file's own order before the swap.
#[test]
fn obj_import_is_converted_to_clockwise_with_outward_normals() {
    // One triangle, counter-clockwise about +Z as OBJ authors it.
    let src = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
    let mesh = Mesh3D::from_obj(src).expect("parses");
    assert!(
        mesh.normals.iter().all(|n| n.z > 0.99),
        "normals must point out of the face (+Z): {:?}",
        mesh.normals
    );
    assert_godot_clockwise("obj triangle", &mesh);
}
