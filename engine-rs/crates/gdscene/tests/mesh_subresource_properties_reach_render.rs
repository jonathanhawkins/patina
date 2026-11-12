//! pat-s3rrv.199: authored primitive-mesh properties survive to render time.
//!
//! A `mesh = SubResource("id")` reference was resolved to a bare
//! `primitive:<Type>` marker, and the sub-resource's authored properties were
//! dropped on the floor. Every primitive therefore rendered at its default
//! size: a `[sub_resource type="TorusMesh"] inner_radius = 0.25` drew Godot's
//! 0.5 instead, in both the runner and the editor.
//!
//! The fix mirrors the `__shape__*` pass that already flattens collision
//! geometry onto the node, plus a `?key=value` suffix on the marker — the
//! editor resolves a mesh from the marker string alone and **caches by it**, so
//! two tori differing only in radius would otherwise collide in that cache.

use gdscene::packed_scene::{
    add_packed_scene_to_tree, percent_decode_marker_value, percent_encode_marker_value, PackedScene,
};
use gdscene::render_server_3d::RenderServer3DAdapter;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const AUTHORED: &str = r#"[gd_scene format=3]

[sub_resource type="TorusMesh" id="Torus_1"]
inner_radius = 0.25
outer_radius = 2.0
rings = 8
ring_segments = 6

[node name="World" type="Node3D"]

[node name="Ring" type="MeshInstance3D" parent="."]
mesh = SubResource("Torus_1")
"#;

/// A sub-resource that authors nothing, to pin that the marker is unchanged.
const BARE: &str = r#"[gd_scene format=3]

[sub_resource type="SphereMesh" id="Sphere_1"]

[node name="World" type="Node3D"]

[node name="Ball" type="MeshInstance3D" parent="."]
mesh = SubResource("Sphere_1")
"#;

fn tree_for(tscn: &str, node: &str) -> (SceneTree, gdscene::node::NodeId) {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let id = tree.find_child(root, node, true).expect("node exists");
    (tree, id)
}

fn prop(tree: &SceneTree, id: gdscene::node::NodeId, key: &str) -> Variant {
    tree.get_node(id).expect("node").get_property(key)
}

/// Reads a numeric property without pinning `Int` vs `Float`.
///
/// A `.tscn` writes `rings = 8` and `inner_radius = 0.25`; which Variant arm a
/// bare integer lands in is the scene parser's business, not this bead's, and
/// asserting the arm would couple these tests to it for no benefit.
fn num(tree: &SceneTree, id: gdscene::node::NodeId, key: &str) -> f64 {
    match prop(tree, id, key) {
        Variant::Float(f) => f,
        Variant::Int(i) => i as f64,
        other => panic!("{key} should be numeric, got {other:?}"),
    }
}

/// The authored values land on the node as `__mesh__*`, like `__shape__*`.
#[test]
fn authored_subresource_properties_are_flattened_onto_the_node() {
    let (tree, ring) = tree_for(AUTHORED, "Ring");

    assert_eq!(
        prop(&tree, ring, "__mesh__class"),
        Variant::String("TorusMesh".into()),
        "the sub-resource's class must survive; it is gone by render time"
    );
    assert_eq!(num(&tree, ring, "__mesh__inner_radius"), 0.25);
    assert_eq!(num(&tree, ring, "__mesh__outer_radius"), 2.0);
    assert_eq!(num(&tree, ring, "__mesh__rings"), 8.0);
    assert_eq!(num(&tree, ring, "__mesh__ring_segments"), 6.0);

    // The real `mesh` property keeps its reference, for round-trip and inspector.
    assert_eq!(
        prop(&tree, ring, "mesh"),
        Variant::String("SubResource:Torus_1".into())
    );
}

/// The marker carries the properties too, because the editor has only the string.
#[test]
fn the_marker_carries_the_authored_properties() {
    let (tree, ring) = tree_for(AUTHORED, "Ring");
    let marker = match prop(&tree, ring, "__viewport_mesh__") {
        Variant::String(s) => s,
        other => panic!("expected a marker string, got {other:?}"),
    };

    assert!(
        marker.starts_with("primitive:TorusMesh?"),
        "the class must still lead the marker: {marker}"
    );
    for expected in [
        "inner_radius=0.25",
        "outer_radius=2",
        "rings=8",
        "ring_segments=6",
    ] {
        assert!(
            marker.contains(expected),
            "marker is missing {expected}: {marker}"
        );
    }
}

/// ⚠️ Property maps have no stable iteration order.
///
/// An unsorted marker would differ between runs, which thrashes the editor's
/// path-keyed mesh cache and makes any saved scene or expectation
/// non-deterministic. This is the assertion that the keys are sorted.
#[test]
fn the_marker_is_deterministic() {
    let mut seen = Vec::new();
    for _ in 0..8 {
        let (tree, ring) = tree_for(AUTHORED, "Ring");
        seen.push(match prop(&tree, ring, "__viewport_mesh__") {
            Variant::String(s) => s,
            other => panic!("expected a marker string, got {other:?}"),
        });
    }
    let first = &seen[0];
    for (i, m) in seen.iter().enumerate() {
        assert_eq!(m, first, "marker differed on parse {i}: {m} vs {first}");
    }

    // Sorted, not merely stable.
    let query = first.split_once('?').expect("a query is present").1;
    let keys: Vec<&str> = query
        .split('&')
        .map(|p| p.split_once('=').expect("key=value").0)
        .collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "marker keys must be sorted: {query}");
}

/// The payoff: the geometry the runner builds uses the authored radii.
#[test]
fn the_runner_renders_the_authored_torus_not_the_default_one() {
    let (tree, ring) = tree_for(AUTHORED, "Ring");
    let mesh = RenderServer3DAdapter::resolve_mesh(&tree, ring);

    // (rings+1) * (ring_segments+1); the seam ring is duplicated for UV wrap.
    assert_eq!(
        mesh.vertices.len(),
        (8 + 1) * (6 + 1),
        "the authored segment counts must drive the tessellation"
    );

    let radial = |v: &gdcore::math::Vector3| (v.x * v.x + v.z * v.z).sqrt();
    let max = mesh.vertices.iter().map(radial).fold(f32::MIN, f32::max);
    let min = mesh.vertices.iter().map(radial).fold(f32::MAX, f32::min);
    assert!(
        (max - 2.0).abs() < 1e-3,
        "outer radius should be the authored 2.0, got {max}"
    );
    assert!(
        (min - 0.25).abs() < 1e-3,
        "inner radius should be the authored 0.25, got {min}"
    );

    // Godot's TorusMesh defaults are 0.5/1.0/64/32 — this must not be that mesh.
    assert_ne!(mesh.vertices.len(), (64 + 1) * (32 + 1));
}

/// ⚠️ Before this bead the runner had no torus case at all, and
/// `"primitive:TorusMesh"` matched none of its `contains("sphere")`-style
/// substring guesses — so it fell through to a default CUBE. Not "nothing", as
/// is easy to assume: a silently wrong shape.
#[test]
fn a_torus_is_no_longer_resolved_as_a_cube() {
    let (tree, ring) = tree_for(AUTHORED, "Ring");
    let mesh = RenderServer3DAdapter::resolve_mesh(&tree, ring);

    assert_ne!(
        mesh.vertices.len(),
        gdserver3d::mesh::Mesh3D::cube(1.0).vertices.len(),
        "the torus must not resolve to the fallback cube"
    );
    let ys = mesh
        .vertices
        .iter()
        .map(|v| v.y.abs())
        .fold(f32::MIN, f32::max);
    // ⚠️ Bounded on BOTH sides, and NOT at the tube radius.
    //
    // The tube radius here is (2.0 - 0.25) / 2 = 0.875, but no vertex reaches
    // it: `ring_segments = 6` samples the cross-section every 60 degrees, and
    // the extremum sits at 90. The real maximum is 0.875 * sin(60 deg) =
    // 0.757772 — asserting the continuous maximum fails on correct geometry,
    // which is exactly what it did on the first run.
    //
    // So the bound is stated the way the maths actually constrains it: strictly
    // above a unit cube's 0.5 half-extent (which is what this test exists to
    // exclude) and never above the tube radius, for any tessellation.
    let ys = mesh
        .vertices
        .iter()
        .map(|v| v.y.abs())
        .fold(f32::MIN, f32::max);
    assert!(
        ys > 0.6,
        "expected a torus spanning more in Y than a unit cube's 0.5, got {ys}"
    );
    assert!(
        ys <= 0.875 + 1e-4,
        "no vertex may exceed the tube radius 0.875, got {ys}"
    );
}

/// A sub-resource authoring nothing must leave the marker exactly as it was.
///
/// This is the regression guard for `mesh_sub_resource_resolved_to_primitive`
/// and for every editor scene that spells the marker literally.
#[test]
fn an_unauthored_subresource_leaves_the_marker_bare() {
    let (tree, ball) = tree_for(BARE, "Ball");
    assert_eq!(
        prop(&tree, ball, "__viewport_mesh__"),
        Variant::String("primitive:SphereMesh".into()),
        "no authored properties means no query suffix"
    );
    assert_eq!(
        prop(&tree, ball, "__mesh__class"),
        Variant::String("SphereMesh".into())
    );
}

/// Vector properties flatten component-wise, which is how the resolver addresses
/// them — and it is the reader, not the scene loader, that splits them.
#[test]
fn a_vector_property_flattens_into_components() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="BoxMesh" id="Box_1"]
size = Vector3(2, 1, 3)

[node name="World" type="Node3D"]

[node name="Crate" type="MeshInstance3D" parent="."]
mesh = SubResource("Box_1")
"#;
    let (tree, crate_id) = tree_for(tscn, "Crate");

    // Stored whole on the node...
    assert_eq!(
        prop(&tree, crate_id, "__mesh__size"),
        Variant::Vector3(gdcore::math::Vector3::new(2.0, 1.0, 3.0))
    );
    // ...and split in the marker, where only scalars can travel.
    let marker = match prop(&tree, crate_id, "__viewport_mesh__") {
        Variant::String(s) => s,
        other => panic!("expected a marker, got {other:?}"),
    };
    for expected in ["size.x=2", "size.y=1", "size.z=3"] {
        assert!(
            marker.contains(expected),
            "marker missing {expected}: {marker}"
        );
    }

    // And the geometry actually uses them.
    let mesh = RenderServer3DAdapter::resolve_mesh(&tree, crate_id);
    let max_x = mesh
        .vertices
        .iter()
        .map(|v| v.x.abs())
        .fold(f32::MIN, f32::max);
    let max_z = mesh
        .vertices
        .iter()
        .map(|v| v.z.abs())
        .fold(f32::MIN, f32::max);
    assert!(
        (max_x - 1.0).abs() < 1e-3,
        "a size.x of 2 is a half-extent of 1, got {max_x}"
    );
    assert!(
        (max_z - 1.5).abs() < 1e-3,
        "a size.z of 3 is a half-extent of 1.5, got {max_z}"
    );
}

// ---------------------------------------------------------------------------
// String properties in the marker. A TextMesh's `text` and `font` are the only
// string-valued primitive properties, and the editor resolves a mesh from the
// marker string alone — so they have to survive a `?k=v&k=v` encoding.
// ---------------------------------------------------------------------------

/// Every marker delimiter must survive a round trip.
#[test]
fn marker_delimiters_survive_encoding() {
    for raw in [
        "hello world",
        "a&b",
        "k=v",
        "who?",
        "100%",
        "res://fonts/My Font.ttf",
        "mixed ?&=% all at once",
        "unicode: αβγ 日本語",
        "",
    ] {
        let encoded = percent_encode_marker_value(raw);
        assert!(
            !encoded.contains('&') && !encoded.contains('=') && !encoded.contains('?'),
            "encoded value still contains a delimiter: {encoded}"
        );
        assert_eq!(
            percent_decode_marker_value(&encoded),
            raw,
            "round trip failed for {raw:?} (encoded {encoded})"
        );
    }
}

/// ⚠️ The specific bug a single-pass encoder exists to prevent.
///
/// Escaping with chained `.replace()` calls double-escapes: replacing `%` after
/// `&` turns an already-emitted `%26` back into `%2526`, and the value decodes
/// to `%26` instead of `&`. A literal percent in the input is what exposes it.
#[test]
fn a_literal_percent_is_not_double_escaped() {
    let encoded = percent_encode_marker_value("50% & rising");
    assert_eq!(
        percent_decode_marker_value(&encoded),
        "50% & rising",
        "encoded as {encoded}"
    );
    // `%` is 0x25, `&` is 0x26 — neither may appear as a doubled escape.
    assert!(
        !encoded.contains("%2525") && !encoded.contains("%2526"),
        "double-escaped: {encoded}"
    );
}

/// A marker that was never encoded still reads back as itself.
#[test]
fn a_malformed_escape_passes_through_literally() {
    // `%zz` is not a valid escape, and `%` at the end has no digits after it.
    assert_eq!(percent_decode_marker_value("100%zz"), "100%zz");
    assert_eq!(percent_decode_marker_value("trailing%"), "trailing%");
    assert_eq!(percent_decode_marker_value("plain"), "plain");
}

/// A TextMesh's strings reach the node and the marker.
#[test]
fn text_mesh_strings_reach_both_channels() {
    let tscn = r#"[gd_scene format=3]

[sub_resource type="TextMesh" id="Text_1"]
text = "Hi there"
font = "res://fonts/My Font.ttf"
font_size = 24

[node name="World" type="Node3D"]

[node name="Label" type="MeshInstance3D" parent="."]
mesh = SubResource("Text_1")
"#;
    let (tree, label) = tree_for(tscn, "Label");

    // Typed on the node, for the runner.
    assert_eq!(
        prop(&tree, label, "__mesh__text"),
        Variant::String("Hi there".into())
    );
    assert_eq!(
        prop(&tree, label, "__mesh__font"),
        Variant::String("res://fonts/My Font.ttf".into())
    );
    assert_eq!(num(&tree, label, "__mesh__font_size"), 24.0);

    // Encoded in the marker, for the editor.
    let marker = match prop(&tree, label, "__viewport_mesh__") {
        Variant::String(s) => s,
        other => panic!("expected a marker, got {other:?}"),
    };
    assert!(marker.starts_with("primitive:TextMesh?"), "{marker}");
    assert!(
        !marker.contains("Hi there"),
        "the space must be escaped, not left raw: {marker}"
    );

    // And it decodes back to what was authored.
    let query = marker.split_once('?').expect("a query").1;
    let text = query
        .split('&')
        .find_map(|p| p.strip_prefix("text="))
        .expect("text= is present");
    assert_eq!(percent_decode_marker_value(text), "Hi there");
}
