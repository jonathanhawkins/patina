//! pat-s3rrv.203: an inline `CanvasTexture` reaches the node, and a
//! `PointLight2D`'s `height` reaches the collected light.
//!
//! 🔑 Both halves fail **silently and plausibly**. A `SubResource:<id>` string
//! names a class and an id and carries no channel data, so an unflattened
//! CanvasTexture leaves the light pass with no normal map and the sprite simply
//! renders flat — which is exactly what an unlit-but-correct sprite looks like.
//! Likewise `height` defaults to 0, and a height of 0 shades a flat surface to
//! black rather than erroring.

use gdscene::light2d_scene::collect_lights_2d;
use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A Sprite2D whose `texture` is a CanvasTexture carrying a diffuse and a
/// normal channel, lit by a PointLight2D at the bead's height of 32.
const SCENE: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Texture2D" path="res://brick_diffuse.png" id="1_d"]
[ext_resource type="Texture2D" path="res://brick_normal.png" id="1_n"]

[sub_resource type="CanvasTexture" id="CanvasTexture_1"]
diffuse_texture = ExtResource("1_d")
normal_texture = ExtResource("1_n")
specular_color = Color(1, 0.5, 0.25, 1)
specular_shininess = 0.5
texture_filter = 1

[node name="Root" type="Node2D"]

[node name="Wall" type="Sprite2D" parent="."]
texture = SubResource("CanvasTexture_1")

[node name="Lamp" type="PointLight2D" parent="."]
height = 32.0
position = Vector2(-100, 0)
"#;

/// ⚠️ The control scene. `texture` naming an **ImageTexture** must NOT produce
/// `__canvas_texture__*` companions.
const PLAIN_SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[sub_resource type="ImageTexture" id="ImageTexture_1"]

[node name="Root" type="Node2D"]

[node name="Wall" type="Sprite2D" parent="."]
texture = SubResource("ImageTexture_1")
"#;

fn load(src: &str) -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    let wall = tree
        .all_nodes_in_tree_order()
        .into_iter()
        .find(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == "Wall")
                .unwrap_or(false)
        })
        .expect("no node named Wall");
    (tree, wall)
}

/// The CanvasTexture's body reaches the node, and its channel references are
/// resolved to real paths.
#[test]
fn a_canvas_texture_sub_resource_reaches_the_node() {
    let (tree, wall) = load(SCENE);
    let node = tree.get_node(wall).expect("wall exists");

    assert!(
        matches!(
            node.get_property("__canvas_texture__class"),
            Variant::String(ref s) if s == "CanvasTexture"
        ),
        "the marker must be present — without it the light pass cannot tell a \
         CanvasTexture from an ordinary texture"
    );

    // 🔑 The channels are `ExtResource("id")` in the scene text. Render time has
    // the tree but NOT the PackedScene, so an unresolved id is a dead end: the
    // normal map would be named but unloadable.
    assert!(
        matches!(
            node.get_property("__canvas_texture__normal_texture"),
            Variant::String(ref s) if s == "res://brick_normal.png"
        ),
        "normal_texture must arrive as a resolved PATH, not an ExtResource id; \
         got {:?}",
        node.get_property("__canvas_texture__normal_texture")
    );
    assert!(
        matches!(
            node.get_property("__canvas_texture__diffuse_texture"),
            Variant::String(ref s) if s == "res://brick_diffuse.png"
        ),
        "diffuse_texture must resolve the same way"
    );

    // Scalars come across verbatim, with no per-field knowledge in the pass.
    assert!(
        matches!(
            node.get_property("__canvas_texture__specular_shininess"),
            Variant::Float(f) if (f - 0.5).abs() < 1e-6
        ),
        "specular_shininess must be copied verbatim"
    );
    assert!(
        matches!(
            node.get_property("__canvas_texture__texture_filter"),
            Variant::Int(1)
        ),
        "texture_filter must be copied verbatim"
    );

    // The real property keeps its reference so the scene still round-trips.
    assert!(
        matches!(node.get_property("texture"), Variant::String(ref s) if s.contains("CanvasTexture_1")),
        "the `texture` property itself must be left alone"
    );
}

/// ⚠️ **The discriminator this pass exists to respect.** `texture` on a Sprite2D
/// usually names an ImageTexture, and stamping one under a CanvasTexture prefix
/// would tell the light pass a normal map exists when none does.
///
/// Normal-mapped shading of a surface whose "normals" are really albedo does not
/// error — it produces plausible-looking nonsense, so nothing downstream would
/// report this.
#[test]
fn an_ordinary_texture_does_not_produce_canvas_texture_companions() {
    let (tree, wall) = load(PLAIN_SCENE);
    let node = tree.get_node(wall).expect("wall exists");

    assert!(
        matches!(node.get_property("__canvas_texture__class"), Variant::Nil),
        "an ImageTexture must not be flattened as a CanvasTexture; got {:?}",
        node.get_property("__canvas_texture__class")
    );
    assert!(
        matches!(
            node.get_property("__canvas_texture__normal_texture"),
            Variant::Nil
        ),
        "and it certainly must not appear to carry a normal map"
    );
}

/// `PointLight2D.height` reaches the collected light.
///
/// 🔑 Its Godot default is 0.0, which is also what an uncollected property
/// reads as — so "the light has height 0" is indistinguishable from "height was
/// never read". Authoring 32 is what separates them.
#[test]
fn point_light_height_reaches_the_collected_light() {
    let (tree, _) = load(SCENE);
    let lights = collect_lights_2d(&tree);
    assert_eq!(lights.len(), 1, "one PointLight2D in the scene");
    assert!(
        (lights[0].height - 32.0).abs() < 1e-6,
        "the authored height must reach the light; 0.0 here means the property \
         is never collected and every normal-mapped surface would go black. \
         got {}",
        lights[0].height
    );
}

/// The control for the test above: a light with no authored `height` keeps
/// Godot's 0.0 default rather than picking up a stray value.
#[test]
fn an_unauthored_height_stays_at_godots_default() {
    const NO_HEIGHT: &str = r#"[gd_scene format=3]

[node name="Root" type="Node2D"]

[node name="Lamp" type="PointLight2D" parent="."]
position = Vector2(-100, 0)
"#;
    let scene = PackedScene::from_tscn(NO_HEIGHT).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");

    let lights = collect_lights_2d(&tree);
    assert_eq!(lights.len(), 1, "one PointLight2D in the scene");
    assert!(
        lights[0].height.abs() < 1e-6,
        "Godot's documented default is 0.0; got {}",
        lights[0].height
    );
}
