//! Acceptance test for pat-s3rrv.21: 2D lighting works for a scene as Godot
//! actually serializes it.
//!
//! Godot stores a `LightOccluder2D`'s geometry in an **`OccluderPolygon2D`
//! sub-resource** (`occluder = SubResource("…")`), not on the node
//! (upstream/godot/scene/2d/light_occluder_2d.cpp). Patina's
//! `collect_occluders_2d` read a bare `polygon` off the node, which is `Nil` for
//! every authored scene — so it returned ZERO occluders and 2D shadows never
//! appeared from a real `.tscn`. The test that guarded this
//! (`light2d_scene_shading_wiring.rs`) hand-builds the node and sets `polygon`
//! itself, so it stayed green over the break.
//!
//! Two more defects on the same path:
//! - lights were placed by their LOCAL `position`, so a torch parented to a
//!   moving `Node2D` lit the origin;
//! - `DirectionalLight2D` was read from a `direction` property **Godot does not
//!   have** — the vector comes from the node's rotation — so every authored
//!   directional light silently used the (0,-1) fallback.
//!
//! This test therefore parses real `.tscn` text and never sets `polygon` on a
//! node directly.

use gdcore::math::{Color, Vector2};
use gdrender2d::light2d::CanvasAmbient;
use gdrender2d::renderer::FrameBuffer;
use gdscene::light2d_scene::{apply_scene_lights, collect_lights_2d, collect_occluders_2d};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

/// Instances `tscn` under a fresh tree's root, as the runtime does.
fn load(tscn: &str) -> SceneTree {
    let scene = PackedScene::from_tscn(tscn).expect("parse tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance scene");
    tree
}

/// A wall and a light, each under a **translated** parent, exactly as Godot
/// writes them: the occluder geometry lives in a sub-resource and the world
/// placement comes from the parents.
///
/// World layout after the parent offsets: the light sits at (50,10) and the wall
/// spans x=[40,60] at y=50 — so (50,30) is lit and (50,80) is behind the wall.
const SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="OccluderPolygon2D" id="Occ_1"]
closed = false
polygon = PackedVector2Array(0, 0, 20, 0)

[node name="World" type="Node2D"]

[node name="LightRig" type="Node2D" parent="."]
position = Vector2(50, 0)

[node name="Torch" type="PointLight2D" parent="LightRig"]
position = Vector2(0, 10)
color = Color(1, 1, 1, 1)
energy = 2.0
range = 500.0
light_mask = 1

[node name="WallRig" type="Node2D" parent="."]
position = Vector2(40, 50)

[node name="Wall" type="LightOccluder2D" parent="WallRig"]
occluder = SubResource("Occ_1")
occluder_light_mask = 1
"#;

/// **The acceptance test.** The occluder's geometry arrives from the
/// sub-resource, both nodes are placed by their parents' world transforms, and
/// the pixels behind the wall are measurably darker than the lit side.
#[test]
fn light2d_occluder_subresource_shades_scene() {
    let tree = load(SCENE);

    // (a) The sub-resource reached the node. Without the loader flattening it,
    //     this is empty and every assertion below would pass or fail for the
    //     wrong reason.
    let occluders = collect_occluders_2d(&tree);
    assert_eq!(
        occluders.len(),
        1,
        "the OccluderPolygon2D sub-resource must produce exactly one occluder; \
         reading a bare `polygon` off the node yields none for a real .tscn"
    );

    // (b) Its points are in WORLD space — offset by the WallRig parent, not the
    //     raw (0,0)..(20,0) the sub-resource stores.
    let pts = &occluders[0].points;
    assert_eq!(pts.len(), 2, "two vertices, got {pts:?}");
    assert!(
        (pts[0].x - 40.0).abs() < 1e-3 && (pts[0].y - 50.0).abs() < 1e-3,
        "first vertex must be the parent-offset world point (40,50), got {:?}",
        pts[0]
    );
    assert!(
        (pts[1].x - 60.0).abs() < 1e-3 && (pts[1].y - 50.0).abs() < 1e-3,
        "second vertex must be (60,50), got {:?}",
        pts[1]
    );

    // (c) The light is placed by its parent too: (50,0) + (0,10).
    let lights = collect_lights_2d(&tree);
    assert_eq!(lights.len(), 1, "one PointLight2D");
    assert!(
        (lights[0].position.x - 50.0).abs() < 1e-3 && (lights[0].position.y - 10.0).abs() < 1e-3,
        "the light must sit at its GLOBAL position (50,10) — a local read puts \
         it at (0,10) and lights the wrong world point; got {:?}",
        lights[0].position
    );

    // (d) The end of the pipeline: shaded pixels.
    let mut fb = FrameBuffer::new(100, 100, Color::new(1.0, 1.0, 1.0, 1.0));
    apply_scene_lights(&tree, &mut fb, CanvasAmbient::default(), 1);
    let lit = fb.get_pixel(50, 30);
    let shadowed = fb.get_pixel(50, 80);
    assert!(
        lit.r > shadowed.r + 0.05,
        "pixels behind the occluder must be measurably darker than the lit side \
         (lit {} vs shadowed {}) — this is the shadow that never appeared for an \
         authored scene",
        lit.r,
        shadowed.r
    );
}

/// A `DirectionalLight2D` takes its vector from the node's ROTATION, so a scene
/// that rotates it 90° shades the opposite face from an unrotated one.
#[test]
fn directional_light2d_direction_comes_from_node_rotation() {
    const UNROTATED: &str = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Sun" type="DirectionalLight2D" parent="."]
color = Color(1, 1, 1, 1)
energy = 1.0
"#;
    // 90° in radians. Godot writes `rotation` on Node2D in radians.
    const ROTATED: &str = r#"[gd_scene format=3]

[node name="World" type="Node2D"]

[node name="Sun" type="DirectionalLight2D" parent="."]
rotation = 1.5707963
color = Color(1, 1, 1, 1)
energy = 1.0
"#;

    let flat = collect_lights_2d(&load(UNROTATED));
    let turned = collect_lights_2d(&load(ROTATED));
    assert_eq!(flat.len(), 1, "one directional light");
    assert_eq!(turned.len(), 1, "one directional light");

    // Unrotated points down -Y (Godot's zero-rotation direction).
    let d0 = flat[0].direction;
    assert!(
        d0.x.abs() < 1e-3 && (d0.y + 1.0).abs() < 1e-3,
        "an unrotated DirectionalLight2D must point (0,-1), got {d0:?}"
    );
    // Rotated 90° sweeps it to +X.
    let d1 = turned[0].direction;
    assert!(
        (d1.x - 1.0).abs() < 1e-3 && d1.y.abs() < 1e-3,
        "rotating the node 90° must turn the light to (1,0) — reading a \
         nonexistent `direction` property leaves BOTH at (0,-1); got {d1:?}"
    );
    // The decisive comparison: the two scenes must not agree.
    assert!(
        (d0.x - d1.x).abs() > 0.5 || (d0.y - d1.y).abs() > 0.5,
        "the rotated and unrotated lights must differ, or the rotation is being \
         ignored ({d0:?} vs {d1:?})"
    );
}

/// A hand-built node that sets `polygon` directly still works — the
/// sub-resource path is an addition, not a replacement.
#[test]
fn a_bare_polygon_on_the_node_still_collects() {
    use gdscene::node::Node;
    use gdvariant::Variant;

    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mut occ = Node::new("Wall", "LightOccluder2D");
    occ.set_property(
        "polygon",
        Variant::PackedVector2Array(vec![Vector2::new(40.0, 50.0), Vector2::new(60.0, 50.0)]),
    );
    tree.add_child(root, occ).unwrap();

    let occluders = collect_occluders_2d(&tree);
    assert_eq!(
        occluders.len(),
        1,
        "the bare-property fallback must keep working for hand-built trees"
    );
}
