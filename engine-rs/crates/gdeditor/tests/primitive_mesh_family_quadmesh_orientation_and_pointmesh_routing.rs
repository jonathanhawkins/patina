//! pat-s3rrv.198 — the PrimitiveMesh family: QuadMesh stops being an alias for
//! PlaneMesh, PointMesh becomes reachable, and RibbonTrailMesh exists at all.
//!
//! Godot ground truth: `doc/classes/QuadMesh.xml` (inherits **PlaneMesh**, and
//! overrides `size` to `Vector2(1, 1)` and `orientation` to `FACE_Z` = 2),
//! `doc/classes/PlaneMesh.xml` (`size` `Vector2(2, 2)`, `orientation` `FACE_Y`
//! = 1, plus `center_offset` / `subdivide_width` / `subdivide_depth`),
//! `doc/classes/PointMesh.xml` (inherits PrimitiveMesh, no members),
//! `doc/classes/RibbonTrailMesh.xml` (`size` 1.0, `sections` 5,
//! `section_length` 0.2, `section_segments` 3, `shape` SHAPE_CROSS = 1) and
//! `scene/resources/3d/primitive_meshes.cpp` for the ribbon geometry.
//!
//! 🔑 The defect this bead exists for is a SHARED MATCH ARM. Every dispatcher
//! wrote `"PlaneMesh" | "QuadMesh" => …`, which is wrong for whichever of the
//! two it is not currently pretending to be — and the two viewports disagreed
//! about which one that was. The 3D renderers gave both PlaneMesh's 2x2 XZ
//! quad, so a QuadMesh rendered double size in the wrong plane; the 2D viewport
//! gave both QuadMesh's 1x1, so a PlaneMesh drew at half size. A test that only
//! checked one class would pass on the bug from either side, so both are
//! asserted here against each other.
//!
//! ⚠️ SCOPE — this file currently covers the half of the bead that is
//! implementable without the three dispatcher files, which are leased to
//! another agent working the sibling bead pat-s3rrv.199 (TorusMesh / TextMesh /
//! TubeTrailMesh in those same match arms). Covered now: the 2D viewport size
//! split, and the RibbonTrailMesh geometry port. The ClassDB registrations and
//! the 3D `.tscn` routing clauses land when those leases drop.

use gdeditor::viewport_2d_render::{collect_canvas_items, Render2DKind};
use gdscene::node::{Node, NodeId};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdserver3d::extra_meshes::{PointMesh, RibbonTrailMesh, RibbonTrailShape};
use gdvariant::Variant;

/// `collect_canvas_items` gates on `node.is_class("CanvasItem")`, so the
/// ClassDB must be populated or the node never reaches the draw list at all —
/// an empty DB makes every assertion below vacuously "invisible".
fn mesh_instance_2d(primitive: &str, authored_size: Option<(f32, f32)>) -> SceneTree {
    gdobject::class_db::register_2d_classes();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let m = tree
        .add_child(root, Node::new("Mesh", "MeshInstance2D"))
        .unwrap();
    let node = tree.get_node_mut(m).unwrap();
    node.set_property(
        "__viewport_mesh__",
        Variant::String(format!("primitive:{primitive}")),
    );
    if let Some((x, y)) = authored_size {
        node.set_property(
            "__mesh__size",
            Variant::Vector2(gdcore::math::Vector2::new(x, y)),
        );
    }
    tree
}

/// The drawn quad's width and height, from the emitted corner list.
fn drawn_extent(tree: &SceneTree) -> (f32, f32) {
    let verts = collect_canvas_items(tree)
        .into_iter()
        .find_map(|i| match i.kind {
            Render2DKind::Mesh { vertices, .. } => Some(vertices),
            _ => None,
        })
        .expect("a primitive MeshInstance2D must emit Mesh geometry, not a Marker");
    let xs: Vec<f32> = verts.iter().map(|v| v.x).collect();
    let ys: Vec<f32> = verts.iter().map(|v| v.y).collect();
    let w =
        xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
    let h =
        ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
    (w, h)
}

/// **The 2D half of the acceptance.** A PlaneMesh draws 2x2 and a QuadMesh 1x1.
///
/// 🔑 Both are asserted in one test and compared to each other. Asserting only
/// "PlaneMesh is 2x2" would still pass if a later change made QuadMesh 2x2 as
/// well — which is precisely the aliasing bug, restored.
#[test]
fn quadmesh_and_planemesh_draw_at_their_own_sizes_in_2d() {
    let (qw, qh) = drawn_extent(&mesh_instance_2d("QuadMesh", None));
    assert!(
        (qw - 1.0).abs() < 1e-5 && (qh - 1.0).abs() < 1e-5,
        "Godot's QuadMesh is 1x1, drew {qw}x{qh}"
    );

    let (pw, ph) = drawn_extent(&mesh_instance_2d("PlaneMesh", None));
    assert!(
        (pw - 2.0).abs() < 1e-5 && (ph - 2.0).abs() < 1e-5,
        "Godot's PlaneMesh is 2x2, drew {pw}x{ph} — the pre-fix code gave it \
         QuadMesh's 1x1"
    );

    assert!(
        pw > qw,
        "a PlaneMesh must draw LARGER than a QuadMesh ({pw} vs {qw}); equal \
         sizes mean the shared match arm is back"
    );
}

/// Both stay centred on the origin — the size fix must not have moved them.
#[test]
fn the_resized_quads_are_still_centred_on_the_origin() {
    for primitive in ["QuadMesh", "PlaneMesh"] {
        let tree = mesh_instance_2d(primitive, None);
        let verts = collect_canvas_items(&tree)
            .into_iter()
            .find_map(|i| match i.kind {
                Render2DKind::Mesh { vertices, .. } => Some(vertices),
                _ => None,
            })
            .unwrap();
        let sx: f32 = verts.iter().map(|v| v.x).sum();
        let sy: f32 = verts.iter().map(|v| v.y).sum();
        assert!(
            sx.abs() < 1e-5 && sy.abs() < 1e-5,
            "{primitive} corners must straddle the origin, centroid ({sx}, {sy})"
        );
    }
}

/// An authored `__mesh__size` beats either class default, for both classes.
///
/// This is what makes the fix a real size lookup rather than two new hardcoded
/// constants: a sub-resource that sets `size` must win.
#[test]
fn an_authored_mesh_size_overrides_both_class_defaults() {
    let (w, h) = drawn_extent(&mesh_instance_2d("QuadMesh", Some((6.0, 3.0))));
    assert!(
        (w - 6.0).abs() < 1e-5 && (h - 3.0).abs() < 1e-5,
        "authored size ignored on QuadMesh, drew {w}x{h}"
    );

    let (w, h) = drawn_extent(&mesh_instance_2d("PlaneMesh", Some((6.0, 3.0))));
    assert!(
        (w - 6.0).abs() < 1e-5 && (h - 3.0).abs() < 1e-5,
        "authored size ignored on PlaneMesh, drew {w}x{h}"
    );
}

/// A mesh type with no 2D geometry must still fall through rather than draw a
/// wrong quad — the positive control for the two tests above.
#[test]
fn an_unrelated_primitive_still_falls_through_to_marker() {
    let tree = mesh_instance_2d("BoxMesh", None);
    assert!(
        !collect_canvas_items(&tree)
            .iter()
            .any(|i| matches!(i.kind, Render2DKind::Mesh { .. })),
        "a BoxMesh has no 2D quad interpretation; it must not fake one"
    );
}

/// **PointMesh is a single point, not a cube.** The geometry has existed since
/// pat-nzgdu.131 with no caller at all; this pins the shape the dispatchers
/// route to.
#[test]
fn point_mesh_generates_exactly_one_point() {
    let mesh = PointMesh.generate();
    assert_eq!(mesh.vertices.len(), 1, "a PointMesh is ONE vertex");
    assert_eq!(
        mesh.primitive_type,
        gdserver3d::PrimitiveType::Points,
        "a point mesh drawn as triangles renders nothing"
    );
    // The distinguishing property vs. the default-cube fallback it must replace.
    assert!(
        mesh.vertices.len() < gdserver3d::mesh::Mesh3D::cube(1.0).vertices.len(),
        "a PointMesh must not be the placeholder cube"
    );
}

/// **RibbonTrailMesh exists and matches Godot's defaults.**
///
/// ⚠️ `shape` defaults to SHAPE_CROSS (1), not SHAPE_FLAT (0). Transcribed from
/// RibbonTrailMesh.xml rather than assumed from the enum order — the first
/// constant is not the default here.
#[test]
fn ribbon_trail_mesh_matches_godot_defaults_and_is_rigged() {
    let m = RibbonTrailMesh::default();
    assert_eq!(m.sections, 5);
    assert_eq!(m.section_segments, 3);
    assert_eq!(m.shape, RibbonTrailShape::Cross);
    assert_eq!(m.shape.as_int(), 1);
    assert!((m.section_length - 0.2).abs() < 1e-6);
    assert!((m.size - 1.0).abs() < 1e-6);

    let mesh = m.generate();
    // 15 segments -> 16 rows x 4 vertices for the crossed pair.
    assert_eq!(mesh.vertices.len(), 64);
    assert_eq!(mesh.indices.len(), 15 * 12);
    let n = mesh.vertices.len() as u32;
    assert!(mesh.indices.iter().all(|&i| i < n), "index out of range");

    // 🔑 The bone weights are why the resource exists — a trail that cannot be
    // driven by a Skeleton3D is a stick. One blend per vertex, summing to 1.
    assert_eq!(mesh.bone_weights.len(), mesh.vertices.len());
    assert!(mesh
        .bone_weights
        .iter()
        .all(|w| (w[0] + w[1] - 1.0).abs() < 1e-5));
}

/// The FLAT shape really is a different mesh, not the same one relabelled.
#[test]
fn ribbon_trail_flat_and_cross_are_distinct_geometry() {
    let flat = RibbonTrailMesh {
        shape: RibbonTrailShape::Flat,
        ..Default::default()
    }
    .generate();
    let cross = RibbonTrailMesh::default().generate();

    assert_eq!(flat.vertices.len() * 2, cross.vertices.len());
    // The flat ribbon lies in XY; only the crossed one has a Z-displaced sheet.
    assert!(flat.vertices.iter().all(|v| v.z.abs() < 1e-6));
    assert!(
        cross.vertices.iter().any(|v| v.z.abs() > 1e-6),
        "SHAPE_CROSS must add the second, Z-spanning ribbon"
    );
}

// ── ClassDB registration (the acceptance's first clause) ─────────────────────

/// The registered default for `class.property`, or `None` when the class does
/// not report that property at all.
///
/// 🔑 Goes through `get_property_list`, which walks the inheritance chain and
/// resolves overrides, rather than reading one registration directly. That is
/// what a script or the inspector sees, and it is the only way the QuadMesh
/// override is observable — QuadMesh re-declares two of PlaneMesh's members and
/// inherits the rest.
fn registered_default(class: &str, property: &str) -> Option<Variant> {
    gdobject::class_db::get_property_list(class)
        .into_iter()
        .find(|p| p.name == property)
        .map(|p| p.default_value)
}

#[test]
fn the_three_missing_primitive_meshes_are_registered_with_godots_parents() {
    gdobject::class_db::register_3d_classes();

    for (class, parent) in [
        ("PointMesh", "PrimitiveMesh"),
        // ⚠️ QuadMesh's parent is PlaneMesh, NOT PrimitiveMesh — it is defined
        // entirely as a PlaneMesh with two overridden defaults.
        ("QuadMesh", "PlaneMesh"),
        ("RibbonTrailMesh", "PrimitiveMesh"),
    ] {
        assert!(
            gdobject::class_db::class_exists(class),
            "{class} is not registered, so a scene cannot instantiate it"
        );
        assert!(
            gdobject::class_db::is_parent_class(class, parent),
            "{class} must inherit {parent}"
        );
        // All three are still PrimitiveMesh resources further up the chain.
        assert!(
            gdobject::class_db::is_parent_class(class, "PrimitiveMesh"),
            "{class} must reach PrimitiveMesh"
        );
    }
}

/// **The override clause.** QuadMesh and PlaneMesh must report DIFFERENT
/// defaults for the two members QuadMesh overrides.
#[test]
fn quadmesh_overrides_planemeshs_size_and_orientation_in_classdb() {
    gdobject::class_db::register_3d_classes();

    assert_eq!(
        registered_default("PlaneMesh", "size"),
        Some(Variant::Vector2(gdcore::math::Vector2::new(2.0, 2.0))),
        "PlaneMesh.size is Vector2(2, 2)"
    );
    assert_eq!(
        registered_default("QuadMesh", "size"),
        Some(Variant::Vector2(gdcore::math::Vector2::new(1.0, 1.0))),
        "QuadMesh overrides size to Vector2(1, 1); inheriting 2x2 means the \
         override never took"
    );

    // FACE_Y = 1 for PlaneMesh, FACE_Z = 2 for QuadMesh.
    assert_eq!(
        registered_default("PlaneMesh", "orientation"),
        Some(Variant::Int(1))
    );
    assert_eq!(
        registered_default("QuadMesh", "orientation"),
        Some(Variant::Int(2)),
        "QuadMesh faces Z; inheriting PlaneMesh's 1 is the aliasing bug"
    );
}

/// A re-declared member must be reported ONCE, not twice.
///
/// ⚠️ ClassDB silently shadows duplicates rather than failing, so a botched
/// override shows up only as a doubled entry in the property list — which is
/// what the inspector would render.
#[test]
fn quadmesh_reports_each_overridden_member_exactly_once() {
    gdobject::class_db::register_3d_classes();
    let props = gdobject::class_db::get_property_list("QuadMesh");
    for name in ["size", "orientation"] {
        let hits = props.iter().filter(|p| p.name == name).count();
        assert_eq!(
            hits, 1,
            "QuadMesh reports `{name}` {hits} times, expected 1"
        );
    }
    // It still inherits the rest of PlaneMesh rather than replacing it.
    for inherited in ["center_offset", "subdivide_width", "subdivide_depth"] {
        assert!(
            props.iter().any(|p| p.name == inherited),
            "QuadMesh must inherit PlaneMesh's `{inherited}`"
        );
    }
}

/// RibbonTrailMesh's registered defaults match the doc XML, `shape` included.
#[test]
fn ribbon_trail_mesh_registers_godots_defaults() {
    gdobject::class_db::register_3d_classes();
    assert_eq!(
        registered_default("RibbonTrailMesh", "sections"),
        Some(Variant::Int(5))
    );
    assert_eq!(
        registered_default("RibbonTrailMesh", "section_segments"),
        Some(Variant::Int(3))
    );
    // ⚠️ SHAPE_CROSS = 1 is the default, not SHAPE_FLAT = 0.
    assert_eq!(
        registered_default("RibbonTrailMesh", "shape"),
        Some(Variant::Int(1)),
        "shape defaults to SHAPE_CROSS (1); 0 would be a flat ribbon"
    );
    assert_eq!(
        registered_default("RibbonTrailMesh", "size"),
        Some(Variant::Float(1.0))
    );
    // `curve` is a resource reference with no default, but must still be listed
    // or nothing can assign one.
    assert_eq!(
        registered_default("RibbonTrailMesh", "curve"),
        Some(Variant::Nil)
    );

    // The registered defaults and the generator must not drift apart.
    let m = RibbonTrailMesh::default();
    assert_eq!(
        registered_default("RibbonTrailMesh", "sections"),
        Some(Variant::Int(m.sections as i64))
    );
    assert_eq!(
        registered_default("RibbonTrailMesh", "shape"),
        Some(Variant::Int(m.shape.as_int()))
    );
}

/// PointMesh declares no members of its own but still inherits PrimitiveMesh's.
#[test]
fn point_mesh_adds_no_members_but_inherits_primitivemeshs() {
    gdobject::class_db::register_3d_classes();
    let point = gdobject::class_db::get_property_list("PointMesh");
    let base = gdobject::class_db::get_property_list("PrimitiveMesh");
    assert!(!base.is_empty(), "PrimitiveMesh itself must be registered");
    assert_eq!(
        point.len(),
        base.len(),
        "PointMesh.xml declares no <member> of its own, so its property list is \
         exactly PrimitiveMesh's"
    );
}

// ── The 3D half: a real .tscn through render_server_3d's resolution ──────────

/// Loads a `.tscn` and returns the tree, exactly as the runtime would.
fn load_scene(tscn: &str) -> SceneTree {
    gdobject::class_db::register_3d_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

fn node_named(tree: &SceneTree, name: &str) -> NodeId {
    tree.all_nodes_in_tree_order()
        .into_iter()
        .find(|id| tree.get_node(*id).map(|n| n.name()) == Some(name))
        .unwrap_or_else(|| panic!("no node named {name}"))
}

/// A one-MeshInstance3D scene whose `mesh` is a sub-resource of `class`.
fn scene_with_mesh(class: &str) -> String {
    format!(
        r#"[gd_scene load_steps=2 format=3]

[sub_resource type="{class}" id="1"]

[node name="Root" type="Node3D"]

[node name="M" type="MeshInstance3D" parent="."]
mesh = SubResource("1")
"#
    )
}

fn resolved(class: &str) -> gdserver3d::mesh::Mesh3D {
    let tree = load_scene(&scene_with_mesh(class));
    let m = node_named(&tree, "M");
    gdscene::render_server_3d::RenderServer3DAdapter::resolve_mesh(&tree, m)
}

/// **The headline acceptance clause.** A QuadMesh authored in a `.tscn` must
/// resolve to a 1x1 quad in the XY plane facing +Z.
///
/// 🔑 Before pat-s3rrv.198 this produced PlaneMesh's geometry: a 2x2 quad in XZ
/// with an UP normal. Every assertion below fails on that, and the normal check
/// is the one that cannot be satisfied by a size fix alone.
#[test]
fn a_tscn_quadmesh_resolves_to_a_1x1_xy_quad_facing_z() {
    let mesh = resolved("QuadMesh");
    assert_eq!(mesh.vertices.len(), 4, "an unsubdivided quad has 4 corners");

    for v in &mesh.vertices {
        assert!(
            v.x >= -0.5 - 1e-5 && v.x <= 0.5 + 1e-5,
            "x outside [-0.5, 0.5]: {v:?} — a 2x2 span means PlaneMesh's size"
        );
        assert!(
            v.y >= -0.5 - 1e-5 && v.y <= 0.5 + 1e-5,
            "y outside [-0.5, 0.5]: {v:?}"
        );
        assert!(
            v.z.abs() < 1e-5,
            "a FACE_Z quad is flat in Z, got {v:?} — nonzero z means it is \
             still being built as an XZ PlaneMesh"
        );
    }
    // The full extent, not just the bounds: a degenerate quad also fits above.
    let span_x = extent(&mesh, |v| v.x);
    let span_y = extent(&mesh, |v| v.y);
    assert!((span_x - 1.0).abs() < 1e-5, "width {span_x}, expected 1");
    assert!((span_y - 1.0).abs() < 1e-5, "height {span_y}, expected 1");

    for n in &mesh.normals {
        assert!(
            (n.z - 1.0).abs() < 1e-5,
            "QuadMesh faces +Z; got {n:?} (UP means it is still a PlaneMesh)"
        );
    }
}

/// The other side of the same defect: PlaneMesh keeps its own 2x2 XZ geometry.
#[test]
fn a_tscn_planemesh_still_resolves_to_a_2x2_xz_quad_facing_up() {
    let mesh = resolved("PlaneMesh");
    assert_eq!(mesh.vertices.len(), 4);

    for v in &mesh.vertices {
        assert!(v.y.abs() < 1e-5, "a FACE_Y plane is flat in Y, got {v:?}");
    }
    let span_x = extent(&mesh, |v| v.x);
    let span_z = extent(&mesh, |v| v.z);
    assert!((span_x - 2.0).abs() < 1e-5, "width {span_x}, expected 2");
    assert!((span_z - 2.0).abs() < 1e-5, "depth {span_z}, expected 2");

    for n in &mesh.normals {
        assert!((n.y - 1.0).abs() < 1e-5, "PlaneMesh faces +Y, got {n:?}");
    }
}

/// The span of a mesh along one axis.
fn extent(mesh: &gdserver3d::mesh::Mesh3D, f: impl Fn(&gdcore::math::Vector3) -> f32) -> f32 {
    let vals: Vec<f32> = mesh.vertices.iter().map(&f).collect();
    vals.iter().cloned().fold(f32::MIN, f32::max) - vals.iter().cloned().fold(f32::MAX, f32::min)
}

/// **PointMesh routing.** A `.tscn` PointMesh must resolve to its one vertex,
/// not to the placeholder cube every unknown class falls back to.
#[test]
fn a_tscn_pointmesh_resolves_to_one_vertex_not_the_placeholder_cube() {
    let mesh = resolved("PointMesh");
    assert_eq!(
        mesh.vertices.len(),
        1,
        "PointMesh must route to its own geometry; a cube's vertex count here \
         means it fell through to the default"
    );
    assert_eq!(mesh.primitive_type, gdserver3d::PrimitiveType::Points);
}

/// RibbonTrailMesh routes too, and is not the placeholder either.
#[test]
fn a_tscn_ribbontrailmesh_resolves_to_its_own_geometry() {
    let mesh = resolved("RibbonTrailMesh");
    assert_eq!(
        mesh.vertices.len(),
        64,
        "16 rows x 4 vertices for the default crossed ribbon"
    );
    assert!(mesh.vertices.iter().any(|v| v.z.abs() > 1e-6));
}

/// **The negative control.** An authored `size` must still win over the class
/// default, for the class whose default it is not.
///
/// ⚠️ Without this, both tests above would pass against an implementation that
/// hardcoded 1x1 for QuadMesh and 2x2 for PlaneMesh and read nothing at all.
#[test]
fn an_authored_size_overrides_the_quadmesh_default_through_the_tscn() {
    let tscn = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="QuadMesh" id="1"]
size = Vector2(4, 6)

[node name="Root" type="Node3D"]

[node name="M" type="MeshInstance3D" parent="."]
mesh = SubResource("1")
"#;
    let tree = load_scene(tscn);
    let m = node_named(&tree, "M");
    let mesh = gdscene::render_server_3d::RenderServer3DAdapter::resolve_mesh(&tree, m);

    let span_x = extent(&mesh, |v| v.x);
    let span_y = extent(&mesh, |v| v.y);
    assert!(
        (span_x - 4.0).abs() < 1e-5 && (span_y - 6.0).abs() < 1e-5,
        "authored size ignored: got {span_x}x{span_y}, expected 4x6"
    );
    // Still FACE_Z — authoring a size must not reset the orientation.
    for n in &mesh.normals {
        assert!((n.z - 1.0).abs() < 1e-5, "orientation lost, normal {n:?}");
    }
}
