//! pat-0q2kx.41 acceptance: the scene's `OmniLight3D` / `SpotLight3D` actually
//! light the render, and `shadow_enabled` actually casts a shadow.
//!
//! The renderer supported point and spot lights, spot cones and shadow maps all
//! along, and `render_server_3d.rs::sync_light_properties` parsed every property —
//! but that adapter is dead code. The wired path
//! (`viewport_3d_render::scene_directional_lights`) collected **only**
//! `DirectionalLight3D` and never set `shadow_enabled`, so an omni or spot light in
//! a scene contributed nothing but an editor marker cube, and no shadow could ever
//! render in the product.
//!
//! Every assertion here compares two renders that differ by exactly one authored
//! property, so a pass cannot come from the scene being lit some other way. That
//! matters especially because a scene with no lights at all falls back to the
//! editor's preview sun — so "remove the light and it goes dark" would be testing
//! the fallback, not the light. Each pair therefore keeps a directional light in
//! both renders and toggles only the thing under test.

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

/// Mean luminance of the frame. The comparisons below are all "this render is
/// brighter/darker than that one", which needs no projection math and cannot pass
/// by accident: if the property under test is ignored, the two frames are
/// identical and the means are equal.
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

fn render(tree: &SceneTree) -> FrameBuffer {
    render_editor_3d_frame(&Viewport3D::new(W, H), tree)
}

/// The render list and light collector both filter on `is_class(...)`, which reads
/// the ClassDB — without registration nothing is collected and every assertion
/// would pass vacuously.
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

/// A `DirectionalLight3D` shining straight down: identity basis points its −Z along
/// −Z, so a −90° turn about X aims it at −Y.
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

/// **The acceptance test.** An `OmniLight3D` adds light to the scene, and turning on
/// `shadow_enabled` darkens the frame where geometry is occluded.
#[test]
fn scene_lights_and_shadows_render() {
    // -- an OmniLight3D contributes illumination ------------------------------
    //
    // Both renders keep the same dim directional light, so the only difference is
    // the omni light — and the preview-sun fallback is out of the picture in both.
    let without_omni = {
        let (mut tree, root) = new_tree();
        mesh(&mut tree, root, "Cube", "BoxMesh", Vector3::ZERO);
        let s = sun(&mut tree, root, false);
        tree.get_node_mut(s)
            .unwrap()
            .set_property("light_energy", Variant::Float(0.15));
        render(&tree)
    };
    let with_omni = {
        let (mut tree, root) = new_tree();
        mesh(&mut tree, root, "Cube", "BoxMesh", Vector3::ZERO);
        let s = sun(&mut tree, root, false);
        tree.get_node_mut(s)
            .unwrap()
            .set_property("light_energy", Variant::Float(0.15));
        let omni = tree
            .add_child(root, Node::new("Omni", "OmniLight3D"))
            .unwrap();
        node3d::set_position(&mut tree, omni, Vector3::new(0.0, 1.2, 1.2));
        let node = tree.get_node_mut(omni).unwrap();
        node.set_property("omni_range", Variant::Float(8.0));
        node.set_property("light_energy", Variant::Float(4.0));
        render(&tree)
    };

    let (dim, lit) = (mean_luma(&without_omni), mean_luma(&with_omni));
    assert!(
        lit > dim + 0.002,
        "an OmniLight3D in the scene must add illumination: {lit} with it vs {dim} \
         without (identical scenes otherwise)",
    );

    // -- shadow_enabled casts a shadow ---------------------------------------
    //
    // A plane with a box floating above it, lit from straight above. With shadows
    // on, the occluded part of the plane goes darker, so the frame's mean drops.
    let shadow_scene = |shadows: bool| {
        let (mut tree, root) = new_tree();
        mesh(&mut tree, root, "Floor", "PlaneMesh", Vector3::ZERO);
        mesh(
            &mut tree,
            root,
            "Occluder",
            "BoxMesh",
            Vector3::new(0.0, 1.0, 0.0),
        );
        sun(&mut tree, root, shadows);
        render(&tree)
    };
    let (no_shadow, shadowed) = (
        mean_luma(&shadow_scene(false)),
        mean_luma(&shadow_scene(true)),
    );
    assert!(
        shadowed < no_shadow,
        "shadow_enabled must darken the occluded region: mean {shadowed} with \
         shadows vs {no_shadow} without (identical scenes otherwise)",
    );
}

/// A `SpotLight3D` reaches the scene too, and its cone matters: a light aimed away
/// from the geometry contributes less than one aimed at it.
#[test]
fn spot_light_cone_direction_affects_the_frame() {
    let spot_scene = |aimed_at_cube: bool| {
        let (mut tree, root) = new_tree();
        mesh(&mut tree, root, "Cube", "BoxMesh", Vector3::ZERO);
        let s = sun(&mut tree, root, false);
        tree.get_node_mut(s)
            .unwrap()
            .set_property("light_energy", Variant::Float(0.15));

        let spot = tree
            .add_child(root, Node::new("Spot", "SpotLight3D"))
            .unwrap();
        node3d::set_position(&mut tree, spot, Vector3::new(0.0, 2.0, 0.0));
        // Aimed down at the cube, or flipped to point straight up and away.
        let pitch = if aimed_at_cube {
            -std::f32::consts::FRAC_PI_2
        } else {
            std::f32::consts::FRAC_PI_2
        };
        node3d::set_rotation(&mut tree, spot, Vector3::new(pitch, 0.0, 0.0));
        let node = tree.get_node_mut(spot).unwrap();
        node.set_property("spot_range", Variant::Float(8.0));
        node.set_property("spot_angle", Variant::Float(50.0));
        node.set_property("light_energy", Variant::Float(4.0));
        render(&tree)
    };

    let (aimed, away) = (mean_luma(&spot_scene(true)), mean_luma(&spot_scene(false)));
    assert!(
        aimed > away,
        "a SpotLight3D aimed at the geometry must light it more than one aimed \
         away: {aimed} vs {away}",
    );
}
