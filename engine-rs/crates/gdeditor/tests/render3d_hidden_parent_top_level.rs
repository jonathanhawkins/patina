//! pat-mgixk.32 acceptance — the 3D render list honours `is_visible_in_tree` and
//! `top_level` for every collected node and light.
//!
//! # What was wrong
//!
//! Three defects on the Play path, all in how the render list reads the scene:
//!
//! 1. `collect_scene_content` (`viewport_3d.rs`) set
//!    `visible: node3d::is_visible(tree, nid)` — the node's OWN flag. Godot
//!    propagates `visible` down the Node3D chain
//!    (`Node3D::_propagate_visibility_changed`), so hiding a parent must hide
//!    its subtree. `build_render_list` filters on exactly that flag
//!    (`viewport_3d_render.rs:125`), so every child of a hidden parent was drawn.
//! 2. `scene_directional_lights` never checked visibility at all, so hiding the
//!    sun left the scene lit. The omni/spot collector beside it DID check, so
//!    the two light families disagreed about what `visible` meant.
//! 3. `node3d::get_global_transform` had no `top_level` check; it lived in a
//!    separate `get_global_transform_top_level` that nothing outside `node3d.rs`
//!    called. `set_as_top_level` makes a node's transform independent of its
//!    parent, so a node that opts out still moved with its parent.
//!
//! # The path driven
//!
//! ```text
//! .tscn -> PackedScene -> SceneTree
//!   -> game_3d_render_inputs (the Play render path, as `render_game_3d_frame`
//!      calls it) -> build_render_list
//!        -> collect_scene_content / scene_directional_lights
//! ```
//!
//! # Why this asserts on the render list and not on pixels
//!
//! The assertions read `Editor3DRenderInputs` — `items` (with each item's
//! `node_id` and the `global_transform` the pipeline composed) and
//! `scene_lights`. That IS the content of the frame: `items` is exactly what
//! gets rasterized and `scene_lights` is what shades it, and
//! `build_render_list` filters on the `visible` flag these three fixes change
//! (viewport_3d_render.rs:125). Each assertion is exact and each fails at HEAD.
//!
//! ⚠️ Earlier revisions also sampled pixels from `render_game_3d_frame`. Those
//! were removed, in two steps, for reasons worth not repeating:
//!
//! 1. A single sampled pixel smuggles in an unverified claim about where
//!    geometry projects. The mesh was in the render list (asserted here) while
//!    the centre pixel was bit-identical with it shown and hidden — the sample
//!    point simply was not on the geometry.
//! 2. Widening to a whole-frame diff then showed the mesh putting ZERO pixels
//!    on screen at all. That is not this bead's doing: the one fixture in this
//!    crate proven to rasterize a mesh through Play,
//!    `play_uses_scene_camera3d.rs`, is itself red in the working tree under
//!    pat-mgixk.33, whose in-flight marker/cull-mask refactor drops a default
//!    `MeshInstance3D` from `items` (`verifier.log:12601`). Asserting pixels
//!    here would couple this bead's verdict to that refactor.

use gdcore::math::Vector3;
use gdeditor::viewport_3d_render::{
    game_3d_render_inputs, viewport_for_scene_camera, Editor3DRenderInputs,
};
use gdscene::node::NodeId;
use gdscene::node3d;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const W: u32 = 96;
const H: u32 = 72;

/// A hidden-able `Node3D` parent holding a `MeshInstance3D`, lit by one
/// directional light, framed by the scene's own camera two units down +Z.
///
/// ⚠️ The `MeshInstance3D` nodes now carry a real `mesh`, and they must
/// (pat-mgixk.51). An earlier version of this file removed it, reasoning that "a
/// `MeshInstance3D` descends from `GeometryInstance3D`, so `is_pivot_only_class`
/// is false and it is never stripped". **`is_pivot_only_class` no longer
/// exists** — it went with pat-mgixk.33's reverted work — so that guarantee is
/// gone, and a mesh-less `MeshInstance3D` now resolves to the same
/// `PrimitiveShape::Box` placeholder a plain `Node3D` gets. The game path strips
/// those, because Godot draws nothing for a `VisualInstance3D` with no geometry.
///
/// So without a mesh this bead's probe would be absent from the game list for the
/// wrong reason, and "a hidden parent drops its child" would pass vacuously. With
/// one it is genuine content: present when visible, dropped when an ancestor is
/// hidden, which is exactly what this test is for.
///
/// A `Camera3D` with no `transform`/`rotation` has the identity basis, which
/// looks down −Z as in Godot; the light's identity basis likewise points its −Z
/// at the mesh's camera-facing face, so that face is lit and the centre of the
/// frame is not background.
const HIDDEN_PARENT_SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node3D"]

[node name="Group" type="Node3D" parent="."]

[node name="Box" type="MeshInstance3D" parent="Group"]
mesh = "primitive:BoxMesh"

[node name="Sun" type="DirectionalLight3D" parent="."]

[node name="Cam" type="Camera3D" parent="."]
position = Vector3(0, 0, 2)
fov = 60.0
current = true
"#;

/// A mesh with TWO directional lights.
///
/// ⚠️ Two on purpose. `setup_editor_lighting_3d` substitutes a full-energy
/// editor "preview sun" when a scene has NO lights at all, so in a one-light
/// scene hiding the sun would swap in a bright stand-in and the frame could come
/// out no darker — the test would be measuring the fallback instead of this fix.
/// A dim fill keeps the list non-empty while contributing almost nothing.
const HIDDEN_LIGHT_SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node3D"]

[node name="Box" type="MeshInstance3D" parent="."]
mesh = "primitive:BoxMesh"

[node name="Sun" type="DirectionalLight3D" parent="."]

[node name="Fill" type="DirectionalLight3D" parent="."]
light_energy = 0.05

[node name="Cam" type="Camera3D" parent="."]
position = Vector3(0, 0, 2)
fov = 60.0
current = true
"#;

/// A mesh under a parent moved far off-axis. At distance 2 with fov 60 the
/// visible half-width is about 1.54 units, so at +10 X the mesh is outside the
/// frustum entirely — until `top_level` detaches it back onto its own origin.
const TOP_LEVEL_SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node3D"]

[node name="Rig" type="Node3D" parent="."]
position = Vector3(10, 0, 0)

[node name="Box" type="MeshInstance3D" parent="Rig"]
mesh = "primitive:BoxMesh"

[node name="Sun" type="DirectionalLight3D" parent="."]

[node name="Cam" type="Camera3D" parent="."]
position = Vector3(0, 0, 2)
fov = 60.0
current = true
"#;

/// Loads a `.tscn` into a fresh tree the way opening a scene does.
///
/// `collect_scene_content` filters on `is_class("Node3D")`, which reads the
/// ClassDB — without `register_3d_classes` the 3D classes are unknown, the
/// render list comes back empty, and every assertion below would pass vacuously.
fn load(source: &str) -> SceneTree {
    gdobject::class_db::register_3d_classes();
    let scene = PackedScene::from_tscn(source).expect("the .tscn parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("the scene instances");
    tree
}

fn node(tree: &SceneTree, path: &str) -> NodeId {
    tree.get_node_by_path(path)
        .unwrap_or_else(|| panic!("the fixture has {path}"))
}

/// Builds exactly the render inputs pressing Play builds, through the scene's
/// own camera.
fn play_inputs(tree: &SceneTree) -> Editor3DRenderInputs {
    let camera = node(tree, "/root/World/Cam");
    let viewport = viewport_for_scene_camera(tree, camera, W, H);
    game_3d_render_inputs(tree, camera, &viewport)
}

/// Whether the render list contains an item for `target`.
fn drawn(tree: &SceneTree, target: NodeId) -> bool {
    play_inputs(tree)
        .items
        .iter()
        .any(|i| i.node_id == target.raw())
}

/// The world origin the render list composed for `target`.
fn drawn_origin(tree: &SceneTree, target: NodeId) -> Vector3 {
    play_inputs(tree)
        .items
        .iter()
        .find(|i| i.node_id == target.raw())
        .expect("the node is in the render list")
        .global_transform
        .origin
}

// ⚠️ NO pixel assertions here, deliberately — see the module note above. The
// assertions below all read the render list the Play pipeline builds, which IS
// the content of the frame: `items` is what gets rasterized and `scene_lights`
// is what shades it. They are exact, and each one fails at HEAD.

fn approx(a: Vector3, b: Vector3) -> bool {
    (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4 && (a.z - b.z).abs() < 1e-4
}

#[test]
fn render3d_hidden_parent_and_top_level() {
    // ---- (1) hiding a parent Node3D hides its children ---------------------
    {
        let mut tree = load(HIDDEN_PARENT_SCENE);
        let group = node(&tree, "/root/World/Group");
        let box_id = node(&tree, "/root/World/Group/Box");

        assert!(
            drawn(&tree, box_id),
            "precondition: the mesh is in the render list to begin with",
        );

        // Only the PARENT is hidden; the mesh's own flag stays true.
        node3d::set_visible(&mut tree, group, false);
        assert!(
            !drawn(&tree, box_id),
            "a MeshInstance3D under a hidden parent Node3D must be dropped from \
             the render list, and so never rasterized — `visible` propagates in \
             Godot, but `collect_scene_content` read only the node's own flag, \
             and `build_render_list` filters on exactly that \
             (viewport_3d_render.rs:125)",
        );

        // Revealing the parent brings it back, so the walk does not latch.
        node3d::set_visible(&mut tree, group, true);
        assert!(
            drawn(&tree, box_id),
            "revealing the parent restores the child",
        );

        // The mesh's OWN flag still works — the propagation did not replace it.
        node3d::set_visible(&mut tree, box_id, false);
        assert!(
            !drawn(&tree, box_id),
            "a node's own `visible` flag still removes it",
        );
    }

    // ---- (2) a hidden DirectionalLight3D stops lighting the scene ----------
    {
        let mut tree = load(HIDDEN_LIGHT_SCENE);
        let sun = node(&tree, "/root/World/Sun");

        let lights = |t: &SceneTree| play_inputs(t).scene_lights.len();
        assert_eq!(lights(&tree), 2, "precondition: both lights are collected");

        node3d::set_visible(&mut tree, sun, false);
        assert_eq!(
            lights(&tree),
            1,
            "a hidden DirectionalLight3D must be dropped from the light list — \
             `scene_directional_lights` had NO visibility check at all, while the \
             omni/spot collector beside it did, so the two light families \
             disagreed about what `visible` meant",
        );

        // It comes back, so the collector is not dropping it for good.
        node3d::set_visible(&mut tree, sun, true);
        assert_eq!(lights(&tree), 2, "revealing the sun re-collects it");

        // And the rule reaches a light through a hidden ANCESTOR — the
        // propagation half of the fix, not the node's own flag.
        let world = node(&tree, "/root/World");
        node3d::set_visible(&mut tree, world, false);
        assert_eq!(
            lights(&tree),
            0,
            "lights under a hidden ancestor Node3D are hidden too",
        );
    }

    // ---- (3) a top_level child ignores its parent's transform --------------
    {
        let mut tree = load(TOP_LEVEL_SCENE);
        let box_id = node(&tree, "/root/World/Rig/Box");

        // Parented normally the mesh inherits the rig's +10 X.
        let nested = drawn_origin(&tree, box_id);
        assert!(
            approx(nested, Vector3::new(10.0, 0.0, 0.0)),
            "precondition: without top_level the mesh is composed with its \
             parent (got {nested:?})",
        );

        // `top_level` makes the mesh ignore the rig, putting it on its own origin.
        node3d::set_top_level(&mut tree, box_id, true);
        let origin = drawn_origin(&tree, box_id);
        assert!(
            approx(origin, Vector3::ZERO),
            "a top_level node's global transform must ignore its parent chain, so \
             the mesh the viewport draws sits on its OWN local origin — the render \
             list called `get_global_transform`, which had no top_level check, so \
             a node that opted out of its parent moved with it anyway (got \
             {origin:?})",
        );

        // The flag is read every frame, not cached at insertion.
        node3d::set_top_level(&mut tree, box_id, false);
        assert!(
            approx(drawn_origin(&tree, box_id), Vector3::new(10.0, 0.0, 0.0)),
            "clearing top_level re-composes the mesh with its parent",
        );
    }
}

/// `is_visible_in_tree` stops at a non-Node3D ancestor, as Godot's
/// `Node3D::is_visible_in_tree` does: it climbs `data.parent`, which is
/// `Object::cast_to<Node3D>(get_parent())` and so null the moment the parent is
/// not a Node3D.
///
/// Worth its own test because the SceneTree root is a `Window`: if the walk did
/// not stop at a non-Node3D, every 3D node's visibility would hang off a node
/// from a different family.
const NON_NODE3D_ANCESTOR_SCENE: &str = r#"[gd_scene format=3]

[node name="World" type="Node3D"]

[node name="Plain" type="Node" parent="."]

[node name="Group" type="Node3D" parent="Plain"]

[node name="Box" type="MeshInstance3D" parent="Plain/Group"]
mesh = "primitive:BoxMesh"
"#;

#[test]
fn render3d_visibility_walk_stops_at_a_non_node3d_ancestor() {
    let mut tree = load(NON_NODE3D_ANCESTOR_SCENE);
    let plain = node(&tree, "/root/World/Plain");
    let group = node(&tree, "/root/World/Plain/Group");
    let box_id = node(&tree, "/root/World/Plain/Group/Box");

    assert!(
        node3d::is_visible_in_tree(&tree, box_id),
        "everything starts visible",
    );

    // A plain `Node` is not a Node3D, so it carries no 3D visibility: hiding it
    // must NOT hide the 3D subtree below it.
    node3d::set_visible(&mut tree, plain, false);
    assert!(
        node3d::is_visible_in_tree(&tree, box_id),
        "a hidden non-Node3D ancestor does not hide a 3D subtree: Godot's walk \
         cannot even reach it",
    );

    // ... while the nearest Node3D ancestor still does.
    node3d::set_visible(&mut tree, group, false);
    assert!(
        !node3d::is_visible_in_tree(&tree, box_id),
        "the Node3D parent between them still propagates",
    );
}
