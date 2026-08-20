//! pat-s3rrv.239 — `NavigationMeshGenerator` resolves in ClassDB.
//!
//! Godot ground truth: `doc/classes/NavigationMeshGenerator.xml` (inherits
//! `Object`, `deprecated=""` at the class level) and
//! `modules/navigation_3d/navigation_mesh_generator.cpp`.
//!
//! # What was actually wrong
//!
//! ⚠️ Not "the singleton is missing" — it already worked from script.
//! `gdscript-interop` has dispatched `NavigationMeshGenerator` for some time
//! (its singleton table and `call_navigationmeshgenerator_static`). What was
//! missing was the **ClassDB entry**, so `ClassDB.class_exists(
//! "NavigationMeshGenerator")` answered `false` for a name a script could
//! successfully call, and `gdeditor`'s generated inspector table listed a class
//! ClassDB denied existed.
//!
//! That is the same editor-vs-ClassDB disagreement pat-0q2kx.184 fixed for the
//! native socket family: "exists but is deprecated" and "does not exist" are
//! different answers, and only one of them is true.

use gdobject::class_db::{get_class_info, register_all_classes};
use gdobject::{class_exists, is_parent_class};

/// 🔑 The class resolves, under Godot's parent.
#[test]
fn a_navigation_mesh_generator_is_registered_under_object() {
    register_all_classes();

    assert!(
        class_exists("NavigationMeshGenerator"),
        "a name scripts can already call must exist in ClassDB"
    );

    // ⚠️ `Object`, not `RefCounted`. It is a singleton, and the doc XML carries
    // `deprecated=\"\"` at the class level — Godot keeps it registered so older
    // projects keep loading, which is exactly why it must resolve here.
    assert!(
        is_parent_class("NavigationMeshGenerator", "Object"),
        "NavigationMeshGenerator inherits Object"
    );
    assert!(
        !is_parent_class("NavigationMeshGenerator", "RefCounted"),
        "it is a singleton, not a RefCounted value — a wrong parent still \
         satisfies class_exists and only shows up as a broken chain"
    );
}

/// 🔑 Every registered method is one the interpreter actually dispatches.
///
/// ⚠️ Godot also documents `bake(navigation_mesh, root_node)`, and it is
/// deliberately **absent** here. It walks a scene tree, which lives in
/// `gdscene` — a crate that depends on the interpreter, so the dispatcher
/// cannot reach it. Registering it anyway would make `has_method("bake")`
/// answer `true` while the call fell through and did nothing observable, which
/// is a worse failure than the method being missing: it looks supported.
#[test]
fn b_only_dispatched_methods_are_advertised() {
    register_all_classes();

    let info = get_class_info("NavigationMeshGenerator").expect("registered");
    let names: Vec<&str> = info.methods.iter().map(|m| m.name.as_str()).collect();

    for expected in [
        "parse_source_geometry_data",
        "bake_from_source_geometry_data",
        "clear",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} is dispatched, so it must be advertised. Got {names:?}"
        );
    }

    assert!(
        !names.contains(&"bake"),
        "`bake` must NOT be advertised while nothing dispatches it — a \
         registered-but-undispatched method reports has_method() true and then \
         silently does nothing. Got {names:?}"
    );
}

/// Godot's documented arities, transcribed from the doc XML.
///
/// ⚠️ `parse_source_geometry_data` takes **four** arguments (navigation_mesh,
/// source_geometry_data, root_node, callback) and
/// `bake_from_source_geometry_data` takes **three** — both carry a trailing
/// `callback` that is easy to omit when counting from the common call site.
#[test]
fn c_method_arities_match_the_doc_xml() {
    register_all_classes();

    let info = get_class_info("NavigationMeshGenerator").expect("registered");
    let arity = |name: &str| {
        info.methods
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} registered"))
            .argument_count
    };

    assert_eq!(arity("parse_source_geometry_data"), 4);
    assert_eq!(arity("bake_from_source_geometry_data"), 3);
    assert_eq!(arity("clear"), 1);
}

/// The source-geometry resources it consumes were already registered; this
/// pins that the generator now sits beside them rather than in isolation.
#[test]
fn d_the_source_geometry_resources_are_still_registered() {
    register_all_classes();

    for class in [
        "NavigationMeshSourceGeometryData2D",
        "NavigationMeshSourceGeometryData3D",
    ] {
        assert!(class_exists(class), "{class} must remain registered");
        assert!(is_parent_class(class, "Resource"));
    }
}
