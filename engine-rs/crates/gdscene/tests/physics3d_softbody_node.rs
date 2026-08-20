//! pat-s3rrv.193 acceptance: a `.tscn` SoftBody3D is simulated and its solved
//! vertices reach the mesh the renderer draws.
//!
//! Godot ground truth: `upstream/godot/scene/3d/physics/soft_body_3d.cpp`. The
//! node hands its mesh to the physics server, the server simulates the vertices,
//! and the solved positions are pushed back through a rendering-server handler
//! so the drawn mesh deforms.
//!
//! ⚠️ Before this bead the solver existed and **nothing called it**: a grep for
//! `SoftBody|soft_body` across `gdscene`, `patina-runner` and `gdrender3d`
//! returned zero hits, so a `.tscn` SoftBody3D loaded, reported its class, and
//! sat perfectly rigid forever. That makes "the node registers at all" as much
//! part of this as the sag.
//!
//! ### Two documented deviations from the bead's wording
//!
//! 1. **A 4-vertex plane, not 5x5.** `PlaneMesh` in this engine carries only
//!    `size` — it has no `subdivide_width`/`subdivide_depth` geometry, and
//!    `resolve_mesh` builds every primitive with `::default()`, ignoring
//!    authored properties. ClassDB *does* register both subdivide properties for
//!    `PlaneMesh`, so the inspector surface exists while the geometry does not.
//!    No `.tscn` can express a subdivided plane today, so there is no "centre
//!    vertex" to reach for; the unpinned edge stands in for it. Closing that gap
//!    means adding subdivision to `gdserver3d::PlaneMesh` AND updating an
//!    exhaustive `PlaneMesh { .. }` literal in `gdeditor/src/viewport_3d_render.rs`
//!    — a cross-crate change with its own blast radius across the editor's
//!    pixel tests, which is why it is not folded in here.
//! 2. **`_pinned_points` is set on the node, not in the `.tscn` text**, because
//!    the scene parser has no `PackedInt32Array(...)` literal.

use gdcore::math::Vector3;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdscene::softbody_sync_3d::published;
use gdscene::MainLoop;
use gdvariant::Variant;

const TSCN: &str = r#"[gd_scene format=3]

[node name="World" type="Node3D"]

[node name="Cloth" type="SoftBody3D" parent="."]
mesh_type = "PlaneMesh"
"#;

/// The default `PlaneMesh` is 2x2 on the XZ plane, so its four vertices are the
/// corners at y = 0. Indices 0 and 1 are the `z = -1` edge.
const PINNED: [usize; 2] = [0, 1];
const HANGING: [usize; 2] = [2, 3];

/// Builds the scene and pins one edge, without stepping it.
fn cloth_scene(pin: bool) -> MainLoop {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(TSCN).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    if pin {
        let cloth = tree.find_child(root, "Cloth", true).expect("Cloth exists");
        let pinned: Vec<i32> = PINNED.iter().map(|&i| i as i32).collect();
        tree.get_node_mut(cloth)
            .expect("Cloth node")
            .set_property("_pinned_points", Variant::PackedInt32Array(pinned));
    }

    let mut ml = MainLoop::new(tree);
    ml.register_physics_bodies();
    ml
}

fn cloth_id(ml: &MainLoop) -> gdscene::node::NodeId {
    let root = ml.tree().root_id();
    ml.tree()
        .find_child(root, "Cloth", true)
        .expect("Cloth exists")
}

/// Vertices the physics step published onto the node.
fn published_vertices(ml: &MainLoop) -> Vec<Vector3> {
    match ml
        .tree()
        .get_node(cloth_id(ml))
        .expect("Cloth node")
        .get_property(published::VERTICES)
    {
        Variant::PackedVector3Array(v) => v,
        other => panic!("SoftBody3D should publish solved vertices, got {other:?}"),
    }
}

/// One simulated second, as the acceptance specifies.
fn step_one_second(ml: &mut MainLoop) {
    for _ in 0..60 {
        ml.step(1.0 / 60.0);
    }
}

/// Clause (a): the cloth sags, and the pinned edge does not move.
#[test]
fn a_pinned_softbody_sags_and_publishes_onto_its_node() {
    let mut ml = cloth_scene(true);
    let authored = RenderServer3DAdapter::resolve_mesh(ml.tree(), cloth_id(&ml)).vertices;
    assert_eq!(
        authored.len(),
        4,
        "the default PlaneMesh is a four-corner quad"
    );

    step_one_second(&mut ml);
    let solved = published_vertices(&ml);
    assert_eq!(
        solved.len(),
        authored.len(),
        "one published position per authored vertex"
    );

    for i in PINNED {
        assert_eq!(
            solved[i], authored[i],
            "pinned vertex {i} must not move on any axis"
        );
    }
    for i in HANGING {
        assert!(
            solved[i].y < authored[i].y - 0.05,
            "unpinned vertex {i} must sag past the threshold (authored {}, solved {})",
            authored[i].y,
            solved[i].y
        );
    }

    for (i, v) in solved.iter().enumerate() {
        assert!(
            v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
            "vertex {i} went non-finite: {v:?}"
        );
    }
}

/// Clause (b): the render collection draws the solved mesh, not the authored one.
#[test]
fn the_render_collection_reads_the_sagged_mesh() {
    let mut ml = cloth_scene(true);
    let authored = RenderServer3DAdapter::resolve_mesh(ml.tree(), cloth_id(&ml)).vertices;

    step_one_second(&mut ml);
    let solved = published_vertices(&ml);

    let mut adapter = RenderServer3DAdapter::new(64, 64);
    adapter.render_frame(ml.tree());

    let drawn = adapter
        .instances()
        .iter()
        .find_map(|i| i.mesh.as_ref())
        .expect("the cloth is collected as a mesh instance");

    assert_eq!(
        drawn.vertices, solved,
        "the drawn mesh must carry the solved positions"
    );
    assert_ne!(
        drawn.vertices, authored,
        "the drawn mesh must NOT still be the authored one"
    );
    assert_eq!(
        drawn.indices.len(),
        6,
        "deforming must not drop the quad's two triangles"
    );
}

/// The control for the pin assertions.
///
/// "Vertices 0 and 1 did not move" proves pinning only if something would
/// otherwise have moved them. With nothing pinned the whole sheet falls, so
/// their stillness in the test above is the pin doing work rather than the
/// solver being inert.
#[test]
fn an_unpinned_softbody_falls_as_a_whole() {
    let mut ml = cloth_scene(false);
    let authored = RenderServer3DAdapter::resolve_mesh(ml.tree(), cloth_id(&ml)).vertices;

    step_one_second(&mut ml);
    let solved = published_vertices(&ml);

    for i in 0..authored.len() {
        assert!(
            solved[i].y < authored[i].y - 0.05,
            "with nothing pinned every vertex must fall, but {i} did not \
             (authored {}, solved {})",
            authored[i].y,
            solved[i].y
        );
    }
}
