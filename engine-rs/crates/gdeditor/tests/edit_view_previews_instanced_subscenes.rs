//! The editor's 2D view shows what an instanced sub-scene contains, as Godot's
//! editor does. Found importing the official Godot demo "Dodge the Creeps":
//! `main.tscn` instances its Player and HUD, and the edit view drew neither —
//! the edited tree keeps each instance as a childless placeholder (the shape
//! the saver writes back as `instance=`), and nothing previewed it.
//!
//! The preview must come WITHOUT touching the edited tree, or the saver, undo
//! and the scene dock would see the sub-scene's nodes as the scene's own.

use gdeditor::scene_renderer::{render_scene_with_instance_previews, render_scene_with_zoom_pan_mut};
use gdeditor::texture_cache::TextureCache;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::SceneTree;

const PLAYER: &str = r#"[gd_scene format=3]

[node name="Player" type="Node2D"]

[node name="Body" type="Polygon2D" parent="."]
color = Color(1, 0, 0, 1)
polygon = PackedVector2Array(0, 0, 12, 0, 12, 12, 0, 12)
"#;

fn main_scene(x: f32) -> String {
    format!(
        r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://player.tscn" id="1_p"]

[node name="Main" type="Node2D"]

[node name="Anchor" type="Polygon2D" parent="."]
color = Color(0, 0, 1, 1)
polygon = PackedVector2Array(0, 0, 4, 0, 4, 4, 0, 4)

[node name="Far" type="Polygon2D" parent="."]
color = Color(0, 0, 1, 1)
polygon = PackedVector2Array(200, 100, 204, 100, 204, 104, 200, 104)

[node name="Player" parent="." instance=ExtResource("1_p")]
position = Vector2({x}, 40)
"#
    )
}

fn edited_tree(x: f32) -> SceneTree {
    let scene = PackedScene::from_tscn(&main_scene(x)).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    tree
}

/// Mean x of the pure-red pixels, and how many there are.
fn red_centroid(fb: &gdrender2d::renderer::FrameBuffer) -> (f32, usize) {
    centroid(fb, |c| c.r > 0.9 && c.g < 0.1 && c.b < 0.1)
}

/// Leftmost x of the pure-blue `Anchor` (the scene origin's marker).
fn anchor_x(fb: &gdrender2d::renderer::FrameBuffer) -> f32 {
    (0..fb.width)
        .find(|&x| {
            (0..fb.height).any(|y| {
                let c = fb.get_pixel(x, y);
                c.b > 0.9 && c.r < 0.1 && c.g < 0.1
            })
        })
        .expect("the blue anchor is drawn") as f32
}

fn centroid(
    fb: &gdrender2d::renderer::FrameBuffer,
    keep: impl Fn(gdcore::math::Color) -> bool,
) -> (f32, usize) {
    let mut sum = 0.0;
    let mut n = 0;
    for y in 0..fb.height {
        for x in 0..fb.width {
            let c = fb.get_pixel(x, y);
            if keep(c) {
                sum += x as f32;
                n += 1;
            }
        }
    }
    (if n > 0 { sum / n as f32 } else { f32::NAN }, n)
}

#[test]
fn the_edit_view_draws_an_instanced_subscene_without_changing_the_edited_tree() {
    gdobject::class_db::register_all_classes();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("player.tscn"), PLAYER).unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let mut cache = TextureCache::new(&root);

    let mut tree = edited_tree(60.0);
    let before = tree.node_count();
    let plain = render_scene_with_zoom_pan_mut(&mut tree, None, 320, 200, 1.0, (0.0, 0.0));
    assert_eq!(red_centroid(&plain).1, 0, "precondition: the placeholder alone draws no body");

    let shown = render_scene_with_instance_previews(
        &mut tree, &root, &mut cache, None, 320, 200, 1.0, (0.0, 0.0),
    );
    let (x60, n) = red_centroid(&shown);
    let x60 = x60 - anchor_x(&shown);
    assert!(n > 50, "the Player's body is previewed: {n} red pixels");
    assert_eq!(tree.node_count(), before, "the edited tree is untouched");

    // The outer scene's position override places the preview: +40 in the
    // scene moves it +40 RELATIVE TO THE SCENE at zoom 1. Measured against the
    // blue anchor at the scene origin, because the view auto-frames on the
    // scene's bounds — which include the moved instance — so absolute screen
    // x moves the framing too.
    let mut moved = edited_tree(100.0);
    let shown = render_scene_with_instance_previews(
        &mut moved, &root, &mut cache, None, 320, 200, 1.0, (0.0, 0.0),
    );
    let (x100, _) = red_centroid(&shown);
    let x100 = x100 - anchor_x(&shown);
    assert!(
        ((x100 - x60) - 40.0).abs() < 1.5,
        "moving the instance by 40 moves its preview by 40: {x60} -> {x100}"
    );
}

/// A sub-scene with an AnimatedSprite2D that never `play()`s (its frame comes
/// from the animation step) and a CanvasLayer HUD anchored full-rect.
const SHIP: &str = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://ship.png" id="1"]

[sub_resource type="SpriteFrames" id="1"]
animations = [{
"frames": [{
"duration": 1.0,
"texture": ExtResource("1")
}],
"loop": true,
"name": &"idle",
"speed": 5.0
}]

[node name="Ship" type="Node2D"]

[node name="Sprite" type="AnimatedSprite2D" parent="."]
sprite_frames = SubResource("1")
animation = &"idle"

[node name="HUD" type="CanvasLayer" parent="."]

[node name="Bar" type="ColorRect" parent="HUD"]
anchors_preset = 10
anchor_right = 1.0
offset_bottom = 6.0
color = Color(1, 1, 0, 1)
"#;

const GAME: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://ship.tscn" id="1_s"]

[node name="Main" type="Node2D"]

[node name="Back" type="ColorRect" parent="."]
anchors_preset = 15
anchor_right = 1.0
anchor_bottom = 1.0
color = Color(1, 0, 1, 1)

[node name="Ship" parent="." instance=ExtResource("1_s")]
position = Vector2(50, 50)
"#;

fn count(fb: &gdrender2d::renderer::FrameBuffer, keep: impl Fn(gdcore::math::Color) -> bool) -> (usize, u32, u32) {
    let (mut n, mut min_x, mut max_x) = (0, u32::MAX, 0);
    for y in 0..fb.height {
        for x in 0..fb.width {
            if keep(fb.get_pixel(x, y)) {
                n += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    (n, min_x, max_x)
}

#[test]
fn an_unplayed_animated_sprite_and_a_canvas_layer_hud_are_previewed() {
    gdobject::class_db::register_all_classes();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ship.tscn"), SHIP).unwrap();
    // The game's base resolution: the HUD lays out 120 wide, not panel-wide.
    std::fs::write(
        dir.path().join("project.godot"),
        "config_version=5\n\n[display]\n\nwindow/size/viewport_width=120\nwindow/size/viewport_height=80\n",
    )
    .unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let mut cache = TextureCache::new(&root);
    let green = gdcore::math::Color::new(0.0, 1.0, 0.0, 1.0);
    cache.insert(
        "res://ship.png",
        gdrender2d::texture::Texture2D {
            width: 8,
            height: 8,
            pixels: vec![green; 64],
        },
    );

    let scene = PackedScene::from_tscn(GAME).unwrap();
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, tree_root, &scene).unwrap();
    let fb = render_scene_with_instance_previews(
        &mut tree, &root, &mut cache, None, 400, 300, 1.0, (0.0, 0.0),
    );

    let (ship, _, _) = count(&fb, |c| c.g > 0.9 && c.r < 0.1 && c.b < 0.1);
    // 8x8 drawn; part of it sits under the editor's own label for the `Ship`
    // node, which is drawn at the same spot. Pure green comes from nothing else.
    assert!(
        ship >= 16,
        "the never-played AnimatedSprite2D draws its current frame: {ship} green pixels"
    );
    let (bar, min_x, max_x) = count(&fb, |c| c.r > 0.9 && c.g > 0.9 && c.b < 0.1);
    assert!(bar > 0, "the CanvasLayer HUD is previewed");
    assert!(
        (max_x - min_x + 1) as i32 - 120 <= 1 && 120 - (max_x - min_x + 1) as i32 <= 1,
        "the HUD is laid out in the game's 120-wide base resolution, not the 400-wide panel: \
         spans x {min_x}..={max_x}"
    );
    // ...and so is the edited scene's OWN full-rect background, which sits
    // under the ship (first child, same z: tree order).
    let (back, bmin, bmax) = count(&fb, |c| c.r > 0.9 && c.b > 0.9 && c.g < 0.1);
    assert!(back > 0, "the edited scene's background draws");
    assert!(
        ((bmax - bmin + 1) as i32 - 120).abs() <= 1,
        "the edited scene's root Control anchors to the 120-wide game rect: x {bmin}..={bmax}"
    );
}
