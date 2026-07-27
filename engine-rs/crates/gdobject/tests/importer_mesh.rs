//! pat-0q2kx.258 — `ImporterMesh` and `ImporterMeshInstance3D` are registered.
//!
//! These are Godot's scene-import intermediates: the importer builds them while
//! reading glTF/FBX and converts them to `ArrayMesh` / `MeshInstance3D` at the
//! end. They exist in ClassDB because an `EditorScenePostImport` hook receives
//! the **pre-conversion** tree, so a post-import script writes
//! `node is ImporterMeshInstance3D` and reads surfaces off the mesh — which
//! needs the class chain to resolve. Patina had neither class, so that check
//! could only ever be false.
//!
//! Parents, method names and arities are generated from `extension_api.json`;
//! property defaults come from `doc/classes/ImporterMeshInstance3D.xml`
//! (the dump is not trustworthy for property types/defaults, only the doc XML
//! is).

use gdobject::class_db::{class_exists, get_method_list, get_property_list};
use gdobject::{is_parent_class, register_3d_classes};
use gdvariant::Variant;

/// Registered argument count for `class.method`.
///
/// ⚠️ There is no `method_arity` helper in ClassDB — `get_method_list` is the
/// only query, and `MethodInfo::argument_count` is the field. Assuming a
/// convenience accessor that does not exist is a compile error, not a wrong
/// answer, but it costs a verify round-trip.
fn arity_of(class: &str, method: &str) -> Option<usize> {
    get_method_list(class)
        .into_iter()
        .find(|m| m.name == method)
        .map(|m| m.argument_count)
}

fn default_of(class: &str, property: &str) -> Option<Variant> {
    get_property_list(class)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

/// **The acceptance's registration half**: both classes exist, under the right
/// parents, with the methods a post-import script calls.
#[test]
fn importer_mesh_add_surface_get_mesh() {
    register_3d_classes();

    assert!(class_exists("ImporterMesh"));
    assert!(class_exists("ImporterMeshInstance3D"));

    // ⚠️ ImporterMesh is a **Resource**, not a Mesh. It cannot stand in for one;
    // `get_mesh()` is exactly the conversion that yields a real ArrayMesh.
    assert!(
        is_parent_class("ImporterMesh", "Resource"),
        "ImporterMesh inherits Resource"
    );
    assert!(
        !is_parent_class("ImporterMesh", "Mesh"),
        "ImporterMesh must NOT be a Mesh — that is the whole reason get_mesh() exists"
    );
    // ⚠️ Asserting `is_parent_class(.., "Object")` here would FAIL, and that is
    // a property of the fixture, not of this registration: `register_3d_classes`
    // registers `Resource` with parent `RefCounted` but never registers
    // `RefCounted`/`Node`/`Object` themselves, so the walk stops one link up.
    // The reachable link is what proves the chain is not severed.
    assert!(
        is_parent_class("ImporterMesh", "RefCounted"),
        "Resource's own parent must still resolve — a severed base is the failure \
         mode a direct-parent check alone cannot see"
    );

    // The methods the bead names, plus the surface-reading surface a
    // post-import script uses.
    for (method, arity) in [
        ("add_surface", 7),
        ("get_surface_count", 0),
        ("get_surface_arrays", 1),
        ("get_surface_name", 1),
        ("get_surface_material", 1),
        ("generate_lods", 3),
        ("clear", 0),
    ] {
        assert_eq!(
            arity_of("ImporterMesh", method),
            Some(arity),
            "ImporterMesh.{method} arity"
        );
    }

    // ⚠️ `get_mesh(base_mesh)` takes one optional argument. Registering it as
    // arity 0 — the shape the name suggests — would make a correct call look
    // like an arity error, which is how this would actually be hit.
    assert_eq!(
        arity_of("ImporterMesh", "get_mesh"),
        Some(1),
        "get_mesh takes an optional base ArrayMesh"
    );

    // The conversion target has to exist for get_mesh's contract to mean
    // anything.
    assert!(class_exists("ArrayMesh"));
    assert!(is_parent_class("ArrayMesh", "Mesh"));
}

/// ⚠️ `ImporterMeshInstance3D` inherits **Node3D**, not GeometryInstance3D.
///
/// Parenting it to GeometryInstance3D would look tidier and be wrong: it would
/// silently grant the whole geometry surface, and `get_property_list` would
/// report properties Godot does not. The tell is that Godot gives this class
/// its own `layer_mask` / `cast_shadow` / `visibility_range_*` copies — which
/// it would not need if it inherited them.
#[test]
fn importer_mesh_instance_inherits_node3d_not_geometry_instance() {
    register_3d_classes();

    assert!(is_parent_class("ImporterMeshInstance3D", "Node3D"));
    assert!(
        !is_parent_class("ImporterMeshInstance3D", "GeometryInstance3D"),
        "Godot's parent is Node3D; inheriting GeometryInstance3D would advertise \
         a surface this class does not have"
    );
    // ⚠️ No deeper assertion is possible here, and asserting one would fail:
    // `inheritance_chain` only walks **registered** classes, and
    // `register_3d_classes` registers `Node3D` with parent `Node` without
    // registering `Node` itself — so the chain is exactly
    // `[ImporterMeshInstance3D, Node3D]` and stops. `ImporterMesh` differs:
    // its chain reaches `RefCounted`, which this fixture does register, which
    // is why the far-end check above is asserted there and not here.
}

/// Property defaults from the doc XML, not from what sounds right.
#[test]
fn importer_mesh_instance_property_defaults_match_the_doc_xml() {
    register_3d_classes();

    // ⚠️ `cast_shadow` defaults to 1 (SHADOW_CASTING_SETTING_ON). The enum's
    // zero is OFF, so the "obvious" 0 default would silently stop every
    // imported mesh casting shadows — a lighting bug with no error.
    assert_eq!(
        default_of("ImporterMeshInstance3D", "cast_shadow"),
        Some(Variant::Int(1)),
        "cast_shadow default is ON (1), not the enum's zero"
    );
    assert_eq!(
        default_of("ImporterMeshInstance3D", "layer_mask"),
        Some(Variant::Int(1))
    );
    for f in [
        "visibility_range_begin",
        "visibility_range_begin_margin",
        "visibility_range_end",
        "visibility_range_end_margin",
    ] {
        assert_eq!(
            default_of("ImporterMeshInstance3D", f),
            Some(Variant::Float(0.0)),
            "{f} default"
        );
    }
    assert_eq!(
        default_of("ImporterMeshInstance3D", "visibility_range_fade_mode"),
        Some(Variant::Int(0)),
        "fade mode DISABLED is 0"
    );
    assert!(
        default_of("ImporterMeshInstance3D", "skeleton_path").is_some(),
        "skeleton_path must be registered (NodePath, empty by default)"
    );
    assert!(
        default_of("ImporterMeshInstance3D", "mesh").is_some(),
        "the mesh property is what carries the ImporterMesh through import"
    );
}

/// The full property set, so a later edit cannot quietly drop one. Ten
/// properties, from the doc XML plus `mesh` and `skin`.
#[test]
fn importer_mesh_instance_registers_every_documented_property() {
    register_3d_classes();

    let names: Vec<String> = get_property_list("ImporterMeshInstance3D")
        .into_iter()
        .map(|p| p.name)
        .collect();
    for want in [
        "mesh",
        "skin",
        "skeleton_path",
        "layer_mask",
        "cast_shadow",
        "visibility_range_begin",
        "visibility_range_begin_margin",
        "visibility_range_end",
        "visibility_range_end_margin",
        "visibility_range_fade_mode",
    ] {
        assert!(
            names.iter().any(|n| n == want),
            "missing property {want}; got {names:?}"
        );
    }
}
