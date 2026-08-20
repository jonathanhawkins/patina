//! Runtime navigation-mesh baking (pat-s3rrv.180).
//!
//! A 3D baker has existed in `gdserver3d::navigation` since the port began and
//! had **zero non-test callers**: nothing walked a scene into source geometry,
//! and nothing wrote a baked result anywhere the runtime could read it. This
//! module is that missing middle.
//!
//! # The contract this module writes into
//!
//! `MainLoop::process_navigation` rebuilds the navigation servers from the
//! region nodes on every call, and a `NavigationRegion3D` contributes polygons
//! only when its node carries two companion properties (`main_loop.rs:1927`):
//!
//! * `__navmesh_vertices__` — a `PackedVector3Array`
//! * `__navmesh_polygons__` — an `Array` of `PackedInt32Array` index lists
//!
//! Baking therefore does not need to touch the servers at all: writing those two
//! keys is enough for the next `process_navigation` to pick the region up, which
//! is what makes a bake pathable *in the same run*.
//!
//! ⚠️ **Both keys, always.** `navmesh_polygon_indices` (`main_loop.rs:664`)
//! treats a missing `__navmesh_polygons__` as *one polygon over every vertex*.
//! Writing vertices alone therefore does not fail — it silently produces a
//! single garbage ring threaded through every triangle corner, which paths
//! across nothing and looks like a baker bug rather than a missing key.

use gdcore::math::Vector3;
use gdvariant::Variant;

use crate::node::NodeId;
use crate::scene_tree::SceneTree;

/// Prefix for `NavigationMesh` bake parameters flattened onto the region node.
///
/// 🔑 The parameters live on the **NavigationMesh resource**
/// (`class_db.rs:25509`), not on the region, and `packed_scene` flattens only
/// `vertices`/`polygons` across that boundary. Without a companion key the
/// baker's `agent_max_slope` test runs against a default the scene never chose —
/// the reader is wired and the value still never arrives.
pub const PARAM_PREFIX: &str = "__navmesh_param_";

/// Reads a bake parameter off the region node, or `default` when unset.
///
/// Accepts `Int` as well as `Float`: a `.tscn` writes `agent_max_slope = 60`
/// without a decimal point, and that parses to `Variant::Int`.
pub fn bake_param(tree: &SceneTree, region: NodeId, name: &str, default: f32) -> f32 {
    let key = format!("{PARAM_PREFIX}{name}__");
    match tree.get_node(region).map(|n| n.get_property(&key)) {
        Some(Variant::Float(f)) => f as f32,
        Some(Variant::Int(i)) => i as f32,
        _ => default,
    }
}

/// Every `MeshInstance3D` at or below `root`, in tree order.
fn mesh_instances(tree: &SceneTree, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = tree.get_node(id) else {
            continue;
        };
        if node.class_name() == "MeshInstance3D" {
            out.push(id);
        }
        stack.extend(node.children().iter().copied());
    }
    out
}

/// Appends one mesh surface's world-space triangles to `out`.
///
/// ⚠️ An empty `indices` array means the vertices are already in draw order —
/// that is how a generated primitive arrives. Treating "no indices" as "no
/// geometry" silently drops exactly the meshes a programmatic test scene builds.
fn push_surface(
    out: &mut Vec<Vector3>,
    vertices: &[Vector3],
    indices: &[u32],
    xform: &gdcore::math3d::Transform3D,
) {
    let mut emit = |a: usize, b: usize, c: usize| {
        if let (Some(&va), Some(&vb), Some(&vc)) =
            (vertices.get(a), vertices.get(b), vertices.get(c))
        {
            out.push(xform.xform(va));
            out.push(xform.xform(vb));
            out.push(xform.xform(vc));
        }
    };
    if indices.is_empty() {
        for tri in 0..vertices.len() / 3 {
            emit(tri * 3, tri * 3 + 1, tri * 3 + 2);
        }
    } else {
        for tri in 0..indices.len() / 3 {
            emit(
                indices[tri * 3] as usize,
                indices[tri * 3 + 1] as usize,
                indices[tri * 3 + 2] as usize,
            );
        }
    }
}

/// Walks `root`'s subtree into source geometry, in world space.
///
/// Godot's `parse_source_geometry_data` honours `parsed_geometry_type`; this
/// covers the `MESH_INSTANCES` half, which is what a `NavigationRegion3D` over
/// visual floor geometry needs.
///
/// 🔑 Geometry comes from `RenderServer3DAdapter::resolve_mesh` — the SAME
/// function that decides what a `MeshInstance3D` renders as. Re-deriving the
/// path/`mesh_type` mapping here would let the navmesh and the picture drift
/// apart, and the bug would look like bad pathing rather than two readers
/// disagreeing.
pub fn parse_source_geometry_3d(
    tree: &SceneTree,
    root: NodeId,
) -> gdserver3d::BakeSourceGeometry3D {
    let mut triangles: Vec<Vector3> = Vec::new();
    for id in mesh_instances(tree, root) {
        let mesh = crate::render_server_3d::RenderServer3DAdapter::resolve_mesh(tree, id);
        let xform = crate::node3d::get_global_transform(tree, id);
        // ⚠️ Both the primary arrays AND every extra surface. A multi-surface
        // mesh keeps its first surface at the top level, so walking only
        // `surfaces` finds nothing on a single-surface mesh and walking only the
        // top level silently drops every surface after the first.
        push_surface(&mut triangles, &mesh.vertices, &mesh.indices, &xform);
        for surface in &mesh.surfaces {
            push_surface(&mut triangles, &surface.vertices, &surface.indices, &xform);
        }
    }
    gdserver3d::BakeSourceGeometry3D { triangles }
}

/// Bakes `region`'s navigation mesh from the geometry beneath it.
///
/// Returns the number of polygons baked. Writes `__navmesh_vertices__` and
/// `__navmesh_polygons__` so the next `process_navigation` publishes the region.
pub fn bake_region_3d(tree: &mut SceneTree, region: NodeId) -> usize {
    let mut params = gdserver3d::NavigationMesh3D::new();
    params.cell_size = bake_param(tree, region, "cell_size", params.cell_size);
    params.cell_height = bake_param(tree, region, "cell_height", params.cell_height);
    params.agent_height = bake_param(tree, region, "agent_height", params.agent_height);
    params.agent_radius = bake_param(tree, region, "agent_radius", params.agent_radius);
    params.agent_max_climb = bake_param(tree, region, "agent_max_climb", params.agent_max_climb);
    params.agent_max_slope = bake_param(tree, region, "agent_max_slope", params.agent_max_slope);
    params.region_min_size = bake_param(tree, region, "region_min_size", params.region_min_size);
    params.edge_max_length = bake_param(tree, region, "edge_max_length", params.edge_max_length);

    let source = parse_source_geometry_3d(tree, region);
    let baked = gdserver3d::bake_navigation_mesh(&params, &source);

    // 🔑 Stored MINUS the region's own position, because `process_navigation`
    // adds it straight back (`main_loop.rs:1931` reads
    // `node3d::get_position`, the LOCAL position). Whatever that pass adds is
    // what this subtracts, so the world triangles reconstruct exactly — storing
    // world coordinates here would double the offset for any region not sitting
    // at its parent's origin.
    let origin = crate::node3d::get_position(tree, region);
    let vertices: Vec<Vector3> = baked.vertices.iter().map(|v| *v - origin).collect();
    let polygons: Vec<Variant> = baked
        .polygons
        .iter()
        .map(|p| Variant::PackedInt32Array(p.indices.iter().map(|&i| i as i32).collect()))
        .collect();
    let count = polygons.len();

    if let Some(node) = tree.get_node_mut(region) {
        node.set_property(
            "__navmesh_vertices__",
            Variant::PackedVector3Array(vertices),
        );
        node.set_property("__navmesh_polygons__", Variant::Array(polygons));
    }
    count
}

/// Answers `NavigationRegion2D/3D`'s bake methods, or `None` for anything else.
///
/// Runs ahead of the generic node-method table in `scripting::call_node_method`
/// and declines every other node, the contract `tilemap::call_tilemap_method`
/// follows.
///
/// 🔑 No ClassDB `.method(...)` entry is needed for this to be callable from a
/// script: `interpreter.rs:13973` tries `call_node_method` and returns on
/// `Some(value)` **before** consulting the ClassDB `responds` table, so a
/// name-based arm here is what makes `$Region.bake_navigation_mesh()` resolve.
///
/// ⚠️ Godot's `bake_navigation_mesh(on_thread)` takes an optional bool. The
/// argument is accepted and ignored: this bake is synchronous, and a script that
/// passes `true` must still get a baked region rather than an
/// `UndefinedFunction` for the one-argument form.
pub fn call_nav_region_method(
    tree: &mut SceneTree,
    node_id: NodeId,
    method: &str,
    _args: &[Variant],
) -> Option<Variant> {
    let class = tree.get_node(node_id)?.class_name().to_string();
    match (class.as_str(), method) {
        ("NavigationRegion3D", "bake_navigation_mesh") => {
            bake_region_3d(tree, node_id);
            // Emitted AFTER the properties are written, so a handler that reads
            // the region back sees the baked mesh rather than the previous one.
            // Upstream emits `bake_finished` at the end of the bake for the same
            // reason (`navigation_region_3d.cpp`).
            tree.emit_signal(node_id, "bake_finished", &[]);
            Some(Variant::Nil)
        }
        _ => None,
    }
}
