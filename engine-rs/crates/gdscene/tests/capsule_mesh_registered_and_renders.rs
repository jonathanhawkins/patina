//! pat-nzgdu.160 — `CapsuleMesh` (and its PrimitiveMesh siblings) generate
//! geometry but were absent from ClassDB. It must be registered under
//! `PrimitiveMesh -> Mesh -> Resource` with Godot's default radius=0.5/height=2.0
//! so it can be created via ClassDB, and a default CapsuleMesh must produce real
//! capsule geometry (what a MeshInstance3D renders). Lives in gdscene because it
//! needs both gdobject's ClassDB and gdserver3d's CapsuleMesh.

use gdobject::class_db;
use gdserver3d::CapsuleMesh;
use gdvariant::Variant;

#[test]
fn capsule_mesh_registered_and_renders() {
    class_db::register_3d_classes();

    // (1) Registered with the PrimitiveMesh -> Mesh -> Resource parent chain.
    assert!(
        class_db::class_exists("CapsuleMesh"),
        "CapsuleMesh must be registered in ClassDB"
    );
    assert!(class_db::is_parent_class("CapsuleMesh", "PrimitiveMesh"));
    assert!(class_db::is_parent_class("CapsuleMesh", "Mesh"));
    assert!(class_db::is_parent_class("CapsuleMesh", "Resource"));

    // (2) Instantiable with Godot's defaults radius=0.5 / height=2.0.
    assert!(
        class_db::instantiate("CapsuleMesh").is_some(),
        "ClassDB must be able to instantiate CapsuleMesh"
    );
    let props = class_db::get_property_list("CapsuleMesh");
    let default_of = |name: &str| {
        props
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.default_value.clone())
    };
    match default_of("radius") {
        Some(Variant::Float(v)) => assert!((v - 0.5).abs() < 1e-6, "radius default 0.5, got {v}"),
        other => panic!("radius property missing/wrong: {other:?}"),
    }
    match default_of("height") {
        Some(Variant::Float(v)) => assert!((v - 2.0).abs() < 1e-6, "height default 2.0, got {v}"),
        other => panic!("height property missing/wrong: {other:?}"),
    }

    // (3) A default CapsuleMesh generates real (non-degenerate) capsule geometry
    // — non-empty vertices + triangle indices, which is what MeshInstance3D
    // renders.
    let mesh = CapsuleMesh::default().generate();
    assert!(mesh.vertex_count() > 0, "capsule mesh generates vertices");
    assert!(
        !mesh.indices.is_empty(),
        "capsule mesh generates triangle indices"
    );
    assert!(
        mesh.indices.len() % 3 == 0,
        "triangle index list is a multiple of 3, got {}",
        mesh.indices.len()
    );
}
