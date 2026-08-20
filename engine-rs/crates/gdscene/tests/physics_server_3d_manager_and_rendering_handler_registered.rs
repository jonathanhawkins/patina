//! pat-s3rrv.242 — the two Object-derived physics-server classes, and a handler
//! that actually carries a soft body's solved mesh.
//!
//! `PhysicsServer3DManager` is one of Godot's singletons: it is how a project
//! picks a physics backend (`register_server` / `set_default_server`).
//! `PhysicsServer3DRenderingServerHandler` is the callback object a backend
//! pushes a SoftBody3D's simulated vertices, normals and AABB through on their way
//! to the RenderingServer.
//!
//! ## What was actually missing
//!
//! Less than the bead claims, and it is worth recording. Already present before
//! this change: the handler's ClassDB entry (parent `Object`, `_`-prefixed
//! virtuals), **both** `*Manager` singletons in the interpreter's
//! `BUILTIN_SINGLETONS`, and `call_server_manager_static` implementing
//! `register_server` / `set_default_server` / `get_servers_count`. The soft-body
//! publish path existed too, as `softbody_deformed_mesh` (pat-s3rrv.193).
//!
//! Genuinely missing: the **manager's** ClassDB registration, the handler's
//! **non-underscore** `set_vertex` / `set_normal` / `set_aabb`, and any object a
//! backend could push into — the class was declared with nothing behind it.
//!
//! ## The point of the third test
//!
//! Registering a class and shipping an inert shell is the failure mode this bead
//! exists to avoid, so the last test drives a real solved body through the handler
//! and checks the vertices it *received* are the *simulated* ones — not the
//! authored mesh, which is what an unwired handler would leave behind.

use gdcore::math::Vector3;
use gdobject::class_db;
use gdphysics3d::softbody3d::{SoftBody3D, SoftBodyId3D, DEFAULT_WELD_EPSILON};
use gdscene::softbody_sync_3d::{softbody_deformed_mesh, PhysicsServer3DRenderingServerHandler};
use gdserver3d::mesh::Mesh3D;

const C0: Vector3 = Vector3::new(1.0, 2.0, 3.0);
const C1: Vector3 = Vector3::new(2.0, 2.0, 3.0);
const C2: Vector3 = Vector3::new(1.0, 2.0, 4.0);

fn mesh(vertices: Vec<Vector3>, indices: Vec<u32>) -> Mesh3D {
    Mesh3D {
        vertices,
        indices,
        ..Default::default()
    }
}

/// **The acceptance.** Both classes are in ClassDB, descend from `Object`, and
/// report the methods a backend and a project actually call.
#[test]
fn physics_server_3d_manager_and_rendering_handler_registered() {
    class_db::register_all_classes();

    for name in [
        "PhysicsServer3DManager",
        "PhysicsServer3DRenderingServerHandler",
    ] {
        assert!(class_db::class_exists(name), "{name} must be registered");
        assert!(
            class_db::is_parent_class(name, "Object"),
            "{name} must descend from Object — a registration whose chain does not \
             reach Object answers class_exists while every Object-descendant check \
             fails"
        );
    }

    for m in ["register_server", "set_default_server"] {
        assert!(
            class_db::class_has_method("PhysicsServer3DManager", m),
            "PhysicsServer3DManager must report {m}"
        );
    }

    // ⚠️ The NON-underscore names. The handler was already registered carrying
    // only `_set_vertex` / `_set_normal` / `_set_aabb` — the virtuals a GDExtension
    // backend *overrides*. These are the ones the engine *calls*, and a handler
    // that advertises only the overridable half gives a caller nothing to invoke.
    for m in ["set_vertex", "set_normal", "set_aabb"] {
        assert!(
            class_db::class_has_method("PhysicsServer3DRenderingServerHandler", m),
            "PhysicsServer3DRenderingServerHandler must report {m}"
        );
    }
}

/// The handler receives the SIMULATED mesh, not the authored one.
///
/// This is the anti-shell test: a handler that is registered but never fed leaves
/// the authored positions in place, and every ClassDB assertion above still passes.
#[test]
fn physics_server_3d_manager_and_rendering_handler_registered_carries_solved_vertices() {
    let base = mesh(vec![C0, C1, C2], vec![0, 1, 2]);
    let (mut body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(1),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    // Move one particle so solved and authored positions genuinely differ. Without
    // this the test would pass on a handler that simply echoed the base mesh.
    let moved = Vector3::new(9.0, -9.0, 9.0);
    body.vertices[map[1]].position = moved;

    let mut handler = PhysicsServer3DRenderingServerHandler::new();
    assert!(
        handler.push_softbody(&base, &body, &map),
        "a body describing this mesh must push"
    );

    assert_eq!(
        handler.vertices().len(),
        base.vertices.len(),
        "the handler receives one vertex per VISUAL vertex, not per particle — \
         welding makes those two counts differ"
    );
    assert_eq!(
        handler.vertices()[1],
        moved,
        "the handler must carry the SOLVED position"
    );
    assert_ne!(
        handler.vertices(),
        base.vertices.as_slice(),
        "…and must therefore differ from the authored mesh; an inert handler \
         would leave these equal"
    );

    // It must agree with the established publish path rather than deriving its own.
    let expected = softbody_deformed_mesh(&base, &body, &map).expect("mesh deforms");
    assert_eq!(
        handler.vertices(),
        expected.vertices.as_slice(),
        "the handler must agree with softbody_deformed_mesh — two implementations \
         of the particle-to-vertex weld would be two things to keep correct"
    );

    // The AABB must bound what was actually pushed, including the moved vertex.
    //
    // ⚠️ Compared against min/max directly rather than with `Aabb::contains_point`,
    // which is HALF-OPEN — `point.x < end.x`. A tight bounding box therefore never
    // "contains" the very vertex that defines its maximum face, so asserting
    // containment of an extreme point tests the wrong thing and fails on a correct
    // AABB. (It failed exactly that way first time round.)
    let aabb = handler.aabb();
    let min = aabb.position;
    let max = Vector3::new(
        aabb.position.x + aabb.size.x,
        aabb.position.y + aabb.size.y,
        aabb.position.z + aabb.size.z,
    );
    for v in handler.vertices() {
        assert!(
            v.x >= min.x && v.y >= min.y && v.z >= min.z,
            "set_aabb must bound every pushed vertex; {v:?} is below min {min:?}"
        );
        assert!(
            v.x <= max.x && v.y <= max.y && v.z <= max.z,
            "set_aabb must bound every pushed vertex; {v:?} is above max {max:?}"
        );
    }
    assert!(
        (max.x - moved.x).abs() < 1e-6 || (min.x - moved.x).abs() < 1e-6,
        "the moved vertex is extreme in x, so it must define a face of the box: \
         moved {moved:?}, box {min:?}..{max:?}"
    );
}

/// `set_vertex` addresses by index and does not promise arrival order.
///
/// A handler that pushed with `Vec::push` would silently reorder a backend that
/// batches, which is invisible in a mesh whose vertices happen to be symmetric.
#[test]
fn physics_server_3d_manager_and_rendering_handler_registered_indexes_out_of_order() {
    let mut handler = PhysicsServer3DRenderingServerHandler::new();
    handler.set_vertex(2, C2);
    handler.set_vertex(0, C0);
    handler.set_normal(1, Vector3::new(0.0, 1.0, 0.0));

    assert_eq!(handler.vertices().len(), 3, "the gap at index 1 is kept");
    assert_eq!(handler.vertices()[0], C0);
    assert_eq!(handler.vertices()[2], C2, "a later index is not reordered");
    assert_eq!(handler.normals().len(), 2, "normals grow independently");

    handler.clear();
    assert!(
        handler.vertices().is_empty() && handler.normals().is_empty(),
        "clear() must drop everything so the handler can be reused per frame"
    );
}

/// An empty body pushes nothing and leaves a zero AABB rather than an inverted one.
///
/// An inverted AABB (min > max) reads as "everything is inside" to a naive culler,
/// so the degenerate case is worth pinning.
#[test]
fn physics_server_3d_manager_and_rendering_handler_registered_empty_push_is_a_noop() {
    let base = mesh(Vec::new(), Vec::new());
    let (body, map) = SoftBody3D::from_mesh(
        SoftBodyId3D(2),
        &base.vertices,
        &base.indices,
        DEFAULT_WELD_EPSILON,
    );

    let mut handler = PhysicsServer3DRenderingServerHandler::new();
    assert!(
        !handler.push_softbody(&base, &body, &map),
        "an empty body has nothing to push"
    );
    assert!(handler.vertices().is_empty());
    let aabb = handler.aabb();
    assert!(
        aabb.size.x >= 0.0 && aabb.size.y >= 0.0 && aabb.size.z >= 0.0,
        "a degenerate AABB must be zero-sized, never inverted; got {:?}",
        aabb.size
    );
}
