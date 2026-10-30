//! pat-s3rrv.239 — `NavigationMeshGenerator.clear(navigation_mesh)`.
//!
//! Godot ground truth: `doc/classes/NavigationMeshGenerator.xml` —
//! `clear(navigation_mesh)`, one of the class's four methods, and the one
//! Patina's dispatcher was missing.
//!
//! # ⚠️ Godot returns void; this returns the cleared mesh
//!
//! Godot's `clear` empties a `NavigationMesh` **resource** in place. Patina's
//! navmesh travels through the script boundary as a Dictionary **value**, so
//! there is nothing to mutate in place. Returning `Nil` to match the void
//! signature would leave the caller holding the un-cleared original with no way
//! to notice, so the cleared mesh is returned and the caller rebinds.
//!
//! # ⚠️ Why this is a new file rather than an addition to the existing suite
//!
//! `remaining_singletons_script_reachable.rs` already covers this singleton's
//! bake arms, but it is **red at HEAD**:
//! `every_manager_names_the_server_patina_ships` fails on a
//! `PhysicsServer3DManager` default unrelated to navigation. Adding to that
//! file would adopt its failure as this bead's gate.

use gdscript_interop::interpreter::Interpreter;
use gdvariant::Variant;

fn run_val(src: &str) -> Variant {
    let mut interp = Interpreter::new();
    interp
        .run(src)
        .unwrap_or_else(|e| panic!("script failed: {e:?}\n--- source ---\n{src}"))
        .return_value
        .expect("script returned a value")
}

fn dict(v: Variant) -> gdvariant::Dictionary {
    match v {
        Variant::Dictionary(d) => d.to_dict(),
        other => panic!("expected a navmesh dictionary, got {other:?}"),
    }
}

/// 🔑 **The acceptance clause.** `clear` empties a baked mesh.
#[test]
fn a_clear_empties_a_baked_navmesh() {
    let cleared = run_val(
        "var geo = {\"vertices\": [Vector3(0, 0, 0), Vector3(1, 0, 0), Vector3(0, 0, 1)], \
         \"indices\": [0, 1, 2]}\n\
         var baked = NavigationMeshGenerator.bake_from_source_geometry_data(geo)\n\
         return NavigationMeshGenerator.clear(baked)\n",
    );
    let d = dict(cleared);

    match d.get(Variant::String("polygons".into())) {
        Some(Variant::Array(polys)) => assert!(polys.is_empty(), "polygons emptied, got {polys:?}"),
        other => panic!("polygons must survive as an empty Array, got {other:?}"),
    }
    match d.get(Variant::String("vertices".into())) {
        Some(Variant::PackedVector3Array(v)) => {
            assert!(v.is_empty(), "vertices emptied, got {v:?}")
        }
        other => panic!("vertices must survive as an empty array, got {other:?}"),
    }
}

/// 🔑 The control that gives the clear its meaning.
///
/// ⚠️ Without this, `a_clear_empties_a_baked_navmesh` would also pass against a
/// build whose **bake** produced nothing — "empty after" proves nothing unless
/// it was non-empty before.
#[test]
fn b_the_mesh_was_non_empty_before_the_clear() {
    let baked = run_val(
        "var geo = {\"vertices\": [Vector3(0, 0, 0), Vector3(1, 0, 0), Vector3(0, 0, 1)], \
         \"indices\": [0, 1, 2]}\n\
         return NavigationMeshGenerator.bake_from_source_geometry_data(geo)\n",
    );
    let d = dict(baked);
    match d.get(Variant::String("polygons".into())) {
        Some(Variant::Array(polys)) => {
            assert_eq!(polys.len(), 1, "one triangle in, one polygon out")
        }
        other => panic!("expected polygons, got {other:?}"),
    }
}

/// 🔑 A cleared mesh keeps the SHAPE of a baked one.
///
/// ⚠️ Returning a bare empty dictionary would pass "is it empty?" while making
/// every later `mesh["vertices"]` read answer `Nil` instead of an empty array —
/// a difference that only surfaces in the caller, far from here.
#[test]
fn c_a_cleared_mesh_keeps_both_keys() {
    let cleared = run_val(
        "var mesh = {\"vertices\": [Vector3(0, 0, 0)], \"polygons\": []}\n\
         return NavigationMeshGenerator.clear(mesh)\n",
    );
    let d = dict(cleared);
    assert!(
        d.get(Variant::String("vertices".into())).is_some(),
        "the `vertices` key must still be present, not absent"
    );
    assert!(
        d.get(Variant::String("polygons".into())).is_some(),
        "the `polygons` key must still be present, not absent"
    );
}

/// Clearing an already-empty mesh is a no-op, not an error.
#[test]
fn d_clear_is_idempotent() {
    let twice = run_val(
        "var mesh = {\"vertices\": [Vector3(1, 2, 3)], \"polygons\": []}\n\
         var once = NavigationMeshGenerator.clear(mesh)\n\
         return NavigationMeshGenerator.clear(once)\n",
    );
    let d = dict(twice);
    match d.get(Variant::String("vertices".into())) {
        Some(Variant::PackedVector3Array(v)) => assert!(v.is_empty()),
        other => panic!("expected empty vertices, got {other:?}"),
    }
}
