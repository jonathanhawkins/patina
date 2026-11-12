//! pat-nzgdu.76 acceptance: `GeometryInstance3D`'s per-instance render settings
//! actually change the editor viewport frame.
//!
//! Godot ground truth: `GeometryInstance3D` (`scene/3d/visual_instance_3d.cpp`,
//! `GeometryInstance3D.xml`) controls *how* an instance is drawn —
//! `cast_shadow` (Off / On / DoubleSided / ShadowsOnly), `visibility_range_*`
//! LOD distances, `transparency`, `material_overlay`, `lod_bias`,
//! `extra_cull_margin`, `custom_aabb`.
//!
//! Patina stored every one of these as a node property and registered them in
//! ClassDB (`gdobject::class_db`), so the inspector showed them and a `.tscn`
//! round-tripped them — but **nothing in the render path read them**. A mesh
//! with `cast_shadow = 0` cast a shadow anyway, and one past its
//! `visibility_range_end` was drawn at full detail. The wiring now runs
//!
//! ```text
//! node properties  ->  gdscene::render_server_3d::geometry_instance_settings_for_node
//!   ->  RenderingServer3D::set_geometry_instance_settings
//!   ->  Instance3D.geometry  ->  shadow_map::rasterize_instances_into_shadow (cast_shadow)
//!                             ->  SoftwareRenderer3D::render_frame          (visibility_range)
//! ```
//!
//! Every assertion compares two renders of scenes that differ by exactly one
//! authored property, so a pass cannot come from the scene being lit or culled
//! some other way — if the property were still ignored, the two frames would be
//! byte-identical and every comparison here would fail.

use gdcore::math::Vector3;
use gdeditor::viewport_3d::Viewport3D;
use gdeditor::viewport_3d_render::render_editor_3d_frame;
use gdrender2d::renderer::FrameBuffer;
use gdscene::node::{Node, NodeId};
use gdscene::node3d;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

const W: u32 = 96;
const H: u32 = 72;

/// Mean luminance of the frame — "this render is brighter/darker than that one"
/// needs no projection math and cannot pass by accident.
fn mean_luma(fb: &FrameBuffer) -> f32 {
    if fb.pixels.is_empty() {
        return 0.0;
    }
    let sum: f32 = fb
        .pixels
        .iter()
        .map(|c| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b)
        .sum();
    sum / fb.pixels.len() as f32
}

/// How many pixels differ between two frames.
///
/// `0` is the strong form of "absent from the output": not merely dimmer, but
/// producing a frame indistinguishable from one where the node does not exist.
fn diff_pixels(a: &FrameBuffer, b: &FrameBuffer) -> usize {
    assert_eq!(a.pixels.len(), b.pixels.len(), "frames must be same size");
    a.pixels
        .iter()
        .zip(b.pixels.iter())
        .filter(|(x, y)| x != y)
        .count()
}

fn render(tree: &SceneTree) -> FrameBuffer {
    render_editor_3d_frame(&Viewport3D::new(W, H), tree)
}

/// The render list and light collector both filter on `is_class(...)`, which
/// reads the ClassDB — without registration nothing is collected and every
/// assertion would pass vacuously.
fn new_tree() -> (SceneTree, NodeId) {
    gdobject::class_db::register_3d_classes();
    let mut tree = SceneTree::new();
    let root_id = tree.root_id();
    let root = tree
        .add_child(root_id, Node::new("Root", "Node3D"))
        .unwrap();
    (tree, root)
}

fn mesh(tree: &mut SceneTree, parent: NodeId, name: &str, primitive: &str, at: Vector3) -> NodeId {
    let mut node = Node::new(name, "MeshInstance3D");
    node.set_property("mesh", Variant::String(format!("primitive:{primitive}")));
    let id = tree.add_child(parent, node).unwrap();
    node3d::set_position(tree, id, at);
    id
}

/// A `DirectionalLight3D` shining straight down: identity basis points its −Z
/// along −Z, so a −90° turn about X aims it at −Y.
fn sun(tree: &mut SceneTree, parent: NodeId, shadows: bool) -> NodeId {
    let id = tree
        .add_child(parent, Node::new("Sun", "DirectionalLight3D"))
        .unwrap();
    node3d::set_position(tree, id, Vector3::new(0.0, 4.0, 0.0));
    node3d::set_rotation(
        tree,
        id,
        Vector3::new(-std::f32::consts::FRAC_PI_2, 0.0, 0.0),
    );
    if shadows {
        tree.get_node_mut(id)
            .unwrap()
            .set_property("shadow_enabled", Variant::Bool(true));
    }
    id
}

/// A floor with a box floating above it, lit from straight above with shadows
/// on. `occluder` decides whether the box exists at all and, when it does, which
/// `GeometryInstance3D` properties it carries.
fn floor_and_box(occluder: Option<&[(&str, Variant)]>) -> FrameBuffer {
    let (mut tree, root) = new_tree();
    mesh(&mut tree, root, "Floor", "PlaneMesh", Vector3::ZERO);
    if let Some(props) = occluder {
        let id = mesh(
            &mut tree,
            root,
            "Occluder",
            "BoxMesh",
            Vector3::new(0.0, 1.0, 0.0),
        );
        let node = tree.get_node_mut(id).unwrap();
        for (name, value) in props.iter() {
            node.set_property(name, value.clone());
        }
    }
    sun(&mut tree, root, true);
    render(&tree)
}

/// **The acceptance test.** `cast_shadow = Off` stops the shadow without hiding
/// the mesh, and a mesh past `visibility_range_end` is absent from the frame.
#[test]
fn geometryinstance_cast_shadow_and_visibility_range_consumed() {
    // -- cast_shadow = Off removes the shadow --------------------------------
    //
    // Identical scenes; the box differs only by `cast_shadow`. With the property
    // ignored (the bug) both frames are byte-identical.
    let casting = floor_and_box(Some(&[]));
    let not_casting = floor_and_box(Some(&[("cast_shadow", Variant::Int(0))]));
    let no_box = floor_and_box(None);

    assert!(
        diff_pixels(&casting, &no_box) > 0,
        "the box must actually render and shadow the floor, or the rest of this \
         test is vacuous"
    );
    assert!(
        diff_pixels(&casting, &not_casting) > 0,
        "cast_shadow = Off must change the frame — the render path is still \
         ignoring GeometryInstance3D.cast_shadow"
    );
    assert!(
        mean_luma(&not_casting) > mean_luma(&casting),
        "cast_shadow = Off must remove the shadow, brightening the floor: \
         {} without casting vs {} with (identical scenes otherwise)",
        mean_luma(&not_casting),
        mean_luma(&casting),
    );
    // ⚠️ Off means "casts no shadow", NOT "invisible" — the mesh itself must
    // still be drawn, or this would pass by deleting the box.
    assert!(
        diff_pixels(&not_casting, &no_box) > 0,
        "cast_shadow = Off must still draw the mesh itself"
    );

    // -- visibility_range_end culls the draw ---------------------------------
    //
    // The editor camera sits several units from the origin, so an end of 0.5
    // puts the box far outside its visibility range.
    let culled = floor_and_box(Some(&[
        ("visibility_range_end", Variant::Float(0.5)),
        // Take the shadow out of the picture: the range is a *draw* decision, so
        // a still-casting box would leave its shadow behind and mask the result.
        ("cast_shadow", Variant::Int(0)),
    ]));
    let in_range = floor_and_box(Some(&[("cast_shadow", Variant::Int(0))]));

    assert!(
        diff_pixels(&in_range, &no_box) > 0,
        "with no range limit the box must render, or the cull assertion below \
         is vacuous"
    );
    assert_eq!(
        diff_pixels(&culled, &no_box),
        0,
        "a mesh past visibility_range_end must be absent from the output — the \
         frame must be indistinguishable from one with no box at all"
    );
}

/// `ShadowsOnly` is the mirror image of `Off`: the mesh vanishes from the colour
/// pass but keeps casting. Getting these two backwards is the whole bug class,
/// so both directions are pinned.
#[test]
fn shadows_only_casts_without_drawing_the_mesh() {
    let casting = floor_and_box(Some(&[]));
    let shadows_only = floor_and_box(Some(&[("cast_shadow", Variant::Int(3))]));
    let no_box = floor_and_box(None);

    assert!(
        diff_pixels(&shadows_only, &casting) > 0,
        "ShadowsOnly must stop drawing the mesh, changing the frame"
    );
    assert!(
        diff_pixels(&shadows_only, &no_box) > 0,
        "ShadowsOnly must still cast a shadow, so the frame cannot match one \
         with no box at all"
    );
    assert!(
        mean_luma(&shadows_only) < mean_luma(&no_box),
        "the surviving shadow must darken the floor: {} with a shadows-only box \
         vs {} with no box",
        mean_luma(&shadows_only),
        mean_luma(&no_box),
    );
}

/// `visibility_range_begin` is the near cutoff — Godot's hand-off to a
/// higher-detail LOD sibling. A begin larger than the camera distance hides the
/// mesh just as the far end does.
#[test]
fn visibility_range_begin_hides_meshes_that_are_too_close() {
    let no_box = floor_and_box(None);
    let too_close = floor_and_box(Some(&[
        ("visibility_range_begin", Variant::Float(1_000.0)),
        ("cast_shadow", Variant::Int(0)),
    ]));

    assert_eq!(
        diff_pixels(&too_close, &no_box),
        0,
        "a mesh nearer than visibility_range_begin must not be drawn"
    );
}

/// Fully transparent is invisible. `transparency = 1.0` is the one value that
/// must produce no pixels regardless of how alpha is composited downstream.
#[test]
fn fully_transparent_instance_is_not_drawn() {
    let no_box = floor_and_box(None);
    let invisible = floor_and_box(Some(&[
        ("transparency", Variant::Float(1.0)),
        ("cast_shadow", Variant::Int(0)),
    ]));

    assert_eq!(
        diff_pixels(&invisible, &no_box),
        0,
        "transparency = 1.0 must draw nothing"
    );
}
