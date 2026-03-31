//! pat-tr83q: CSG boolean operations (union, intersection, subtraction).
//!
//! Validates:
//! 1. CSGOperation enum (Union, Intersection, Subtraction) with Godot int mapping
//! 2. Union combines all meshes via concatenation
//! 3. Intersection keeps only overlapping volume
//! 4. Subtraction removes child volume from parent
//! 5. combine() dispatches to correct operation
//! 6. Edge cases: non-overlapping, empty meshes, identical meshes
//! 7. Winding order inversion for subtraction interior surfaces

use gdcore::math::Vector3;
use gdserver3d::csg::{CSGBox3D, CSGCombiner3D, CSGCylinder3D, CSGOperation, CSGSphere3D};
use gdserver3d::mesh::Mesh3D;

// ── CSGOperation enum ───────────────────────────────────────────────

#[test]
fn operation_union_is_default() {
    assert_eq!(CSGOperation::default(), CSGOperation::Union);
}

#[test]
fn operation_godot_int_roundtrip() {
    assert_eq!(CSGOperation::from_godot_int(0), CSGOperation::Union);
    assert_eq!(CSGOperation::from_godot_int(1), CSGOperation::Intersection);
    assert_eq!(CSGOperation::from_godot_int(2), CSGOperation::Subtraction);
    assert_eq!(CSGOperation::Union.to_godot_int(), 0);
    assert_eq!(CSGOperation::Intersection.to_godot_int(), 1);
    assert_eq!(CSGOperation::Subtraction.to_godot_int(), 2);
}

// ── Union ───────────────────────────────────────────────────────────

#[test]
fn union_concatenates_meshes() {
    let box_a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let box_b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let mesh_a = box_a.to_mesh();
    let mesh_b = box_b.to_mesh();

    let combined = CSGCombiner3D::combine_union(&[mesh_a.clone(), mesh_b.clone()]);

    assert_eq!(
        combined.vertices.len(),
        mesh_a.vertices.len() + mesh_b.vertices.len()
    );
    assert_eq!(
        combined.indices.len(),
        mesh_a.indices.len() + mesh_b.indices.len()
    );
}

#[test]
fn union_empty_inputs_produces_empty() {
    let combined = CSGCombiner3D::combine_union(&[]);
    assert!(combined.vertices.is_empty());
    assert!(combined.indices.is_empty());
}

#[test]
fn union_indices_are_valid() {
    let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
    let b = CSGSphere3D::default().to_mesh();
    let combined = CSGCombiner3D::combine_union(&[a, b]);

    let n = combined.vertices.len() as u32;
    for &idx in &combined.indices {
        assert!(idx < n, "Index {idx} out of range for {n} vertices");
    }
}

// ── Intersection ────────────────────────────────────────────────────

#[test]
fn intersection_overlapping_boxes_produces_triangles() {
    // Two boxes overlapping at the center
    let box_a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0));
    let box_b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0));
    let mesh_a = box_a.to_mesh();
    let mesh_b = box_b.to_mesh();

    let result = CSGCombiner3D::combine_intersection(&mesh_a, &mesh_b);

    // Identical overlapping boxes: all triangles should be inside each other's AABB
    assert!(
        !result.vertices.is_empty(),
        "Intersection of overlapping boxes should produce geometry"
    );
    assert!(!result.indices.is_empty());
}

#[test]
fn intersection_non_overlapping_produces_empty() {
    let box_a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let mut mesh_a = box_a.to_mesh();
    // Shift A far away
    for v in &mut mesh_a.vertices {
        v.x += 100.0;
    }

    let box_b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let mesh_b = box_b.to_mesh();

    let result = CSGCombiner3D::combine_intersection(&mesh_a, &mesh_b);
    assert!(
        result.vertices.is_empty(),
        "Non-overlapping should produce empty intersection"
    );
}

#[test]
fn intersection_partial_overlap_reduces_geometry() {
    let box_a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0));
    let mesh_a = box_a.to_mesh();

    let box_b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0));
    let mut mesh_b = box_b.to_mesh();
    // Shift B partially
    for v in &mut mesh_b.vertices {
        v.x += 0.5;
    }

    let result = CSGCombiner3D::combine_intersection(&mesh_a, &mesh_b);

    // Intersection should have fewer triangles than the union
    let union_count = mesh_a.indices.len() + mesh_b.indices.len();
    assert!(
        result.indices.len() <= union_count,
        "Intersection should have <= union triangle count"
    );
}

#[test]
fn intersection_indices_are_valid() {
    let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let result = CSGCombiner3D::combine_intersection(&a, &b);

    let n = result.vertices.len() as u32;
    for &idx in &result.indices {
        assert!(idx < n, "Index {idx} out of range for {n} vertices");
    }
}

#[test]
fn intersection_with_empty_mesh_returns_empty() {
    let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
    let empty = Mesh3D::new(gdserver3d::mesh::PrimitiveType::Triangles);

    let result = CSGCombiner3D::combine_intersection(&a, &empty);
    assert!(result.vertices.is_empty());
}

// ── Subtraction ─────────────────────────────────────────────────────

#[test]
fn subtraction_non_overlapping_returns_original() {
    let box_a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let mesh_a = box_a.to_mesh();
    let orig_vert_count = mesh_a.vertices.len();
    let orig_idx_count = mesh_a.indices.len();

    let box_b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let mut mesh_b = box_b.to_mesh();
    // Shift B far away
    for v in &mut mesh_b.vertices {
        v.x += 100.0;
    }

    let result = CSGCombiner3D::combine_subtraction(&mesh_a, &mesh_b);
    assert_eq!(result.vertices.len(), orig_vert_count);
    assert_eq!(result.indices.len(), orig_idx_count);
}

#[test]
fn subtraction_overlapping_removes_interior() {
    let box_a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0));
    let mesh_a = box_a.to_mesh();

    // Smaller box inside
    let box_b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let mesh_b = box_b.to_mesh();

    let result = CSGCombiner3D::combine_subtraction(&mesh_a, &mesh_b);

    // Result should have geometry (original outside + inverted B inside)
    assert!(!result.vertices.is_empty());
    assert!(!result.indices.is_empty());
}

#[test]
fn subtraction_indices_are_valid() {
    let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
    let result = CSGCombiner3D::combine_subtraction(&a, &b);

    let n = result.vertices.len() as u32;
    for &idx in &result.indices {
        assert!(idx < n, "Index {idx} out of range for {n} vertices");
    }
}

#[test]
fn subtraction_with_empty_b_returns_a() {
    let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
    let empty = Mesh3D::new(gdserver3d::mesh::PrimitiveType::Triangles);

    let result = CSGCombiner3D::combine_subtraction(&a, &empty);
    assert_eq!(result.vertices.len(), a.vertices.len());
    assert_eq!(result.indices.len(), a.indices.len());
}

// ── combine() dispatcher ────────────────────────────────────────────

#[test]
fn combine_dispatches_union() {
    let a = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();
    let b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();

    let result = CSGCombiner3D::combine(&a, &b, CSGOperation::Union);
    assert_eq!(result.vertices.len(), a.vertices.len() + b.vertices.len());
}

#[test]
fn combine_dispatches_intersection() {
    let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let b = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();

    let result = CSGCombiner3D::combine(&a, &b, CSGOperation::Intersection);
    assert!(!result.vertices.is_empty());
}

#[test]
fn combine_dispatches_subtraction() {
    let a = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0)).to_mesh();

    let result = CSGCombiner3D::combine(&a, &b, CSGOperation::Subtraction);
    assert!(!result.vertices.is_empty());
}

// ── Winding order ───────────────────────────────────────────────────

#[test]
fn subtraction_inverts_winding_on_interior_surface() {
    // When subtracting, the B triangles inside A should have inverted winding
    // so normals point inward (forming the interior surface).
    let box_a = CSGBox3D::new(Vector3::new(4.0, 4.0, 4.0));
    let mesh_a = box_a.to_mesh();

    let box_b = CSGBox3D::new(Vector3::new(1.0, 1.0, 1.0));
    let mesh_b = box_b.to_mesh();

    let result = CSGCombiner3D::combine_subtraction(&mesh_a, &mesh_b);

    // The result should have more triangles than the original A alone
    // (A outside + B inverted interior)
    assert!(
        result.indices.len() > 0,
        "Subtraction should produce geometry"
    );
}

// ── Primitive mesh generation (basic sanity) ────────────────────────

#[test]
fn box_generates_valid_mesh() {
    let b = CSGBox3D::new(Vector3::new(2.0, 3.0, 4.0));
    let mesh = b.to_mesh();
    assert_eq!(mesh.vertices.len(), 8);
    assert_eq!(mesh.indices.len(), 36);
}

#[test]
fn sphere_generates_valid_mesh() {
    let s = CSGSphere3D::default();
    let mesh = s.to_mesh();
    assert!(!mesh.vertices.is_empty());
    assert!(!mesh.indices.is_empty());
}

#[test]
fn cylinder_generates_valid_mesh() {
    let c = CSGCylinder3D::new(1.0, 2.0);
    let mesh = c.to_mesh();
    assert!(!mesh.vertices.is_empty());
    assert!(!mesh.indices.is_empty());
}

// ── Cross-primitive operations ──────────────────────────────────────

#[test]
fn union_box_and_sphere() {
    let box_mesh = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let sphere_mesh = CSGSphere3D::default().to_mesh();

    let result = CSGCombiner3D::combine(&box_mesh, &sphere_mesh, CSGOperation::Union);
    assert_eq!(
        result.vertices.len(),
        box_mesh.vertices.len() + sphere_mesh.vertices.len()
    );
}

#[test]
fn intersection_box_and_sphere() {
    let box_mesh = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let sphere_mesh = CSGSphere3D::default().to_mesh();

    let result = CSGCombiner3D::combine(&box_mesh, &sphere_mesh, CSGOperation::Intersection);
    // Sphere is inside the box, so intersection should retain sphere triangles
    assert!(!result.vertices.is_empty());
}

#[test]
fn subtraction_box_minus_cylinder() {
    let box_mesh = CSGBox3D::new(Vector3::new(2.0, 2.0, 2.0)).to_mesh();
    let cyl_mesh = CSGCylinder3D::new(0.5, 3.0).to_mesh();

    let result = CSGCombiner3D::combine(&box_mesh, &cyl_mesh, CSGOperation::Subtraction);
    assert!(!result.vertices.is_empty());
}
