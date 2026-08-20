//! pat-s3rrv.199 acceptance: primitive-mesh subtypes resolve, carry their
//! authored properties, and produce the SAME geometry in the runner and the
//! editor.
//!
//! Godot ground truth: `upstream/godot/scene/resources/3d/primitive_meshes.cpp`
//! and `doc/classes/{TorusMesh,TubeTrailMesh}.xml`.
//!
//! ⚠️ The defect this bead exists for was a **live editor/runtime divergence**.
//! `fixtures/scenes/demo_3d.tscn` ships a `[sub_resource type="TorusMesh"]`; the
//! editor had a `"TorusMesh"` arm and the runner did not. The runner's resolver
//! was a chain of substring guesses (`contains("sphere")`, `contains("plane")`)
//! with no torus case, so the Donut fell through to `Mesh3D::cube(1.0)` — it
//! drew a **cube**, not nothing, which is the worse failure of the two because
//! it looks like geometry.
//!
//! There is now one resolver, `gdserver3d::primitive_mesh::build_primitive`,
//! owning Godot's defaults, and both views call it. These tests compare the two
//! views through **each one's own entry point** rather than by calling the
//! shared resolver twice — that would only prove it agrees with itself, not
//! that either view actually routes through it.
//!
//! ### TextMesh
//!
//! `TextMesh` needs its `text` and a font path — **strings**, which the numeric
//! property lookup could not carry. Rather than resolve it separately in each
//! view (which is how the two diverged in the first place), the shared resolver
//! grew a string channel: typed `__mesh__*` on the node for the runner,
//! percent-encoded into the marker query for the editor, whose only input is
//! that string. Godot falls back to a project default font here; this engine
//! has no such plumbing, so a TextMesh without an authored `font` resolves to
//! nothing rather than to a placeholder that would claim the text rendered.
//!
//! ### Two arithmetic errors in the acceptance text, implemented correctly
//!
//! 1. Vertex count is `(rings+1) * (ring_segments+1)`, not `rings *
//!    ring_segments`: the seam ring is duplicated so UVs can wrap.
//! 2. Bounding radius is `outer_radius`, not `outer_radius + inner_radius` —
//!    the far edge sits at `centre + tube = (outer+inner)/2 + (outer-inner)/2`.

use gdeditor::viewport_3d_render::mesh_for_primitive;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdserver3d::mesh::Mesh3D;
use gdvariant::Variant;

fn scene_from(tscn: &str) -> SceneTree {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    tree
}

fn node(tree: &SceneTree, name: &str) -> NodeId {
    let root = tree.root_id();
    tree.find_child(root, name, true)
        .unwrap_or_else(|| panic!("{name} exists"))
}

/// What the runner would draw.
fn runner_mesh(tree: &SceneTree, id: NodeId) -> Mesh3D {
    RenderServer3DAdapter::resolve_mesh(tree, id)
}

/// What the editor viewport would draw, reached through the editor's own
/// resolution entry point and fed the marker the scene loader produced.
fn editor_mesh(tree: &SceneTree, id: NodeId) -> Mesh3D {
    let marker = match tree
        .get_node(id)
        .expect("node")
        .get_property("__viewport_mesh__")
    {
        Variant::String(s) => s,
        other => panic!("expected a resolved primitive marker, got {other:?}"),
    };
    mesh_for_primitive(&marker)
}

fn radial(v: &gdcore::math::Vector3) -> f32 {
    (v.x * v.x + v.z * v.z).sqrt()
}

fn max_radius(mesh: &Mesh3D) -> f32 {
    mesh.vertices.iter().map(radial).fold(f32::MIN, f32::max)
}

fn min_radius(mesh: &Mesh3D) -> f32 {
    mesh.vertices.iter().map(radial).fold(f32::MAX, f32::min)
}

/// Asserts the two views produced the same geometry, vertex for vertex.
fn assert_views_agree(tree: &SceneTree, id: NodeId, what: &str) -> Mesh3D {
    let runner = runner_mesh(tree, id);
    let editor = editor_mesh(tree, id);

    assert_eq!(
        runner.vertices.len(),
        editor.vertices.len(),
        "{what}: runner and editor disagree on vertex count \
         ({} vs {}) — the two views have drifted apart again",
        runner.vertices.len(),
        editor.vertices.len()
    );
    assert_eq!(
        runner.indices.len(),
        editor.indices.len(),
        "{what}: runner and editor disagree on index count"
    );
    // Counts can match while the shapes differ, so compare the positions too.
    assert_eq!(
        runner.vertices, editor.vertices,
        "{what}: same vertex count but different positions"
    );
    runner
}

// ---------------------------------------------------------------------------
// The shipped fixture — the exact scene the bead cites.
// ---------------------------------------------------------------------------

fn demo_3d() -> String {
    let path = format!(
        "{}/../../fixtures/scenes/demo_3d.tscn",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

/// `demo_3d.tscn`'s Donut renders as a torus in the runner, in both views.
#[test]
fn the_shipped_demo_scene_renders_its_torus_in_the_runner() {
    let tree = scene_from(&demo_3d());
    let donut = node(&tree, "Donut");

    let mesh = assert_views_agree(&tree, donut, "demo_3d.tscn Donut");
    assert!(!mesh.vertices.is_empty(), "the Donut must carry geometry");

    // ⚠️ The regression this bead exists for: a default cube stood in for the
    // torus. A cube has 24 vertices and a hole of zero.
    assert_ne!(
        mesh.vertices.len(),
        Mesh3D::cube(1.0).vertices.len(),
        "the Donut must not resolve to the fallback cube"
    );
    assert!(
        min_radius(&mesh) > 0.1,
        "a torus has a hole: its nearest vertex stands off the axis, got {}",
        min_radius(&mesh)
    );

    // The fixture authors no properties, so Godot's documented defaults apply:
    // inner 0.5 / outer 1.0 / rings 64 / ring_segments 32.
    assert_eq!(mesh.vertices.len(), (64 + 1) * (32 + 1));
    assert!((max_radius(&mesh) - 1.0).abs() < 1e-3);
}

// ---------------------------------------------------------------------------
// Authored properties — the "not defaults" half of the acceptance, which the
// shipped fixture cannot carry because its TorusMesh authors nothing.
// ---------------------------------------------------------------------------

const AUTHORED: &str = r#"[gd_scene format=3]

[sub_resource type="TorusMesh" id="Torus_A"]
inner_radius = 0.25
outer_radius = 2.0
rings = 10
ring_segments = 6

[sub_resource type="TubeTrailMesh" id="Tube_A"]
radius = 1.5
radial_steps = 6
sections = 2
section_length = 0.5
section_rings = 2

[node name="World" type="Node3D"]

[node name="Ring" type="MeshInstance3D" parent="."]
mesh = SubResource("Torus_A")

[node name="Trail" type="MeshInstance3D" parent="."]
mesh = SubResource("Tube_A")
"#;

/// The torus uses its authored radii and segment counts, in both views.
#[test]
fn an_authored_torus_uses_its_authored_values_in_both_views() {
    let tree = scene_from(AUTHORED);
    let ring = node(&tree, "Ring");
    let mesh = assert_views_agree(&tree, ring, "authored TorusMesh");

    assert_eq!(
        mesh.vertices.len(),
        (10 + 1) * (6 + 1),
        "authored rings/ring_segments must drive the tessellation"
    );
    assert!(
        (max_radius(&mesh) - 2.0).abs() < 1e-3,
        "authored outer_radius 2.0, got {}",
        max_radius(&mesh)
    );
    assert!(
        (min_radius(&mesh) - 0.25).abs() < 1e-3,
        "authored inner_radius 0.25, got {}",
        min_radius(&mesh)
    );

    // Explicitly NOT Godot's defaults, which is the clause being proven.
    assert_ne!(mesh.vertices.len(), (64 + 1) * (32 + 1));
}

/// A TubeTrailMesh sub-resource produces geometry, in both views.
#[test]
fn an_authored_tube_trail_produces_geometry_in_both_views() {
    let tree = scene_from(AUTHORED);
    let trail = node(&tree, "Trail");
    let mesh = assert_views_agree(&tree, trail, "authored TubeTrailMesh");

    assert!(
        !mesh.vertices.is_empty(),
        "TubeTrailMesh must have geometry"
    );

    // body (section_rings * sections + 1) * (radial_steps + 1) = 5 * 7 = 35,
    // plus two caps of (radial_steps + 2) = 8 each.
    assert_eq!(mesh.vertices.len(), 35 + 8 + 8);
    assert!(
        (max_radius(&mesh) - 1.5).abs() < 1e-3,
        "authored radius 1.5, got {}",
        max_radius(&mesh)
    );

    let max_y = mesh.vertices.iter().map(|v| v.y).fold(f32::MIN, f32::max);
    let min_y = mesh.vertices.iter().map(|v| v.y).fold(f32::MAX, f32::min);
    assert!(
        ((max_y - min_y) - 1.0).abs() < 1e-3,
        "depth is section_length * sections = 0.5 * 2, got {}",
        max_y - min_y
    );
}

/// The wiring requirement, swept across the family.
///
/// ⚠️ Compared through each view's OWN entry point. Calling the shared resolver
/// twice would pass even if one view still had a private match — which is
/// exactly the state this bead is fixing.
#[test]
fn the_two_views_agree_on_every_primitive_subtype() {
    for (class, authored) in [
        ("BoxMesh", "size = Vector3(2, 1, 3)"),
        ("SphereMesh", "radius = 0.75\nrings = 12"),
        ("CylinderMesh", "top_radius = 0.2\nheight = 3.0"),
        ("CapsuleMesh", "radius = 0.4\nheight = 2.5"),
        ("PlaneMesh", "size = Vector2(4, 6)"),
        ("PrismMesh", "size = Vector3(1, 2, 3)"),
        ("TorusMesh", "inner_radius = 0.1\nrings = 7"),
        ("TubeTrailMesh", "radius = 0.9\nsections = 3"),
    ] {
        let tscn = format!(
            r#"[gd_scene format=3]

[sub_resource type="{class}" id="R"]
{authored}

[node name="World" type="Node3D"]

[node name="M" type="MeshInstance3D" parent="."]
mesh = SubResource("R")
"#
        );
        let tree = scene_from(&tscn);
        let m = node(&tree, "M");
        let mesh = assert_views_agree(&tree, m, class);
        assert!(
            !mesh.vertices.is_empty(),
            "{class} must produce geometry in both views"
        );
    }
}

/// A TextMesh sub-resource produces real glyph geometry in both views.
///
/// ⚠️ Its strings travel through **two different channels** — typed on the node
/// for the runner, percent-encoded in the marker for the editor — so this is
/// the one primitive where the two views could disagree through an encoding
/// bug rather than a resolver bug. Comparing them is the point.
#[test]
fn a_text_mesh_produces_glyph_geometry_in_both_views() {
    let font = format!(
        "{}/../gdresource/test_assets/Cantarell-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    let tscn = format!(
        r#"[gd_scene format=3]

[sub_resource type="TextMesh" id="Text_1"]
text = "Hi there"
font = "{font}"
font_size = 24

[node name="World" type="Node3D"]

[node name="Label" type="MeshInstance3D" parent="."]
mesh = SubResource("Text_1")
"#
    );

    let tree = scene_from(&tscn);
    let label = node(&tree, "Label");
    let mesh = assert_views_agree(&tree, label, "TextMesh");

    assert!(
        !mesh.vertices.is_empty(),
        "TextMesh must produce glyph geometry, not an empty mesh"
    );
    // Extruded text has depth, which is what separates it from a flat outline.
    let depth = mesh.vertices.iter().map(|v| v.z).fold(f32::MIN, f32::max)
        - mesh.vertices.iter().map(|v| v.z).fold(f32::MAX, f32::min);
    assert!(
        depth > 0.0,
        "the text must be extruded along Z, got {depth}"
    );

    // The space in "Hi there" survived the marker encoding: had it truncated
    // the value, the editor would have drawn only "Hi" and the two views would
    // already have disagreed above — but assert the run's width directly too.
    let width = mesh.vertices.iter().map(|v| v.x).fold(f32::MIN, f32::max);
    assert!(width > 0.0, "the run advances along X, got {width}");
}

/// A TextMesh with no font draws no glyphs, in either view.
///
/// Godot falls back to the project's default font; this engine has no such
/// plumbing, so the resolver returns `None` and each caller supplies its own
/// placeholder. ⚠️ Deliberately NOT asserting the two placeholders are
/// identical — the runner's and the editor's fallbacks are separate choices
/// made outside the shared resolver, and claiming they match would be a guess
/// I cannot check here. What matters is that neither invents glyphs.
#[test]
fn a_text_mesh_without_a_font_draws_no_glyphs() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="TextMesh" id="Text_1"]
text = "Hi"

[node name="World" type="Node3D"]

[node name="Label" type="MeshInstance3D" parent="."]
mesh = SubResource("Text_1")
"#;
    let tree = scene_from(tscn);
    let label = node(&tree, "Label");

    // A placeholder box is a couple of dozen vertices; extruded glyphs are
    // hundreds. The gap is wide enough that this cannot be ambiguous.
    for (view, mesh) in [
        ("runner", runner_mesh(&tree, label)),
        ("editor", editor_mesh(&tree, label)),
    ] {
        assert!(
            mesh.vertices.len() < 100,
            "{view} drew {} vertices for a fontless TextMesh — that is glyph \
             geometry, not a placeholder",
            mesh.vertices.len()
        );
    }
}
