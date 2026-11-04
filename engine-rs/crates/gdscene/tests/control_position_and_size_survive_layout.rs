//! A Control's `position`/`size` are views of its offsets in Godot: setting
//! either rewrites the offsets. Patina's layout read only the offsets and wrote
//! position/size back every frame, so a rect that arrived through
//! position/size — authored in a `.tscn` without offsets, or assigned by a
//! script — was replaced by the (zero) offset rect on the next layout. A
//! ColorRect placed that way vanished from frame 2 of Play.
//!
//! Driven through repeated `render_play_frame` calls, as Play is.

use gdcore::math::Vector2;
use gdscene::canvas_render::render_play_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::texture_cache::TextureCache;
use gdscene::{LifecycleManager, NodeId, SceneTree};
use gdvariant::Variant;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Plate" type="ColorRect" parent="."]
position = Vector2(10, 10)
size = Vector2(10, 10)
color = Color(1, 0, 0, 1)

[node name="Anchored" type="ColorRect" parent="."]
anchor_left = 1.0
anchor_right = 1.0
offset_left = -10.0
offset_top = 40.0
offset_bottom = 50.0
color = Color(0, 1, 0, 1)
"#;

fn boot() -> (SceneTree, NodeId) {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(SCENE).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    LifecycleManager::enter_tree(&mut tree, main);
    (tree, main)
}

fn frame(tree: &mut SceneTree) -> gdrender2d::renderer::FrameBuffer {
    let mut cache = TextureCache::new(".");
    let settings = gdcore::project_settings::ProjectSettings::default();
    render_play_frame(tree, 100, 60, &mut cache, &settings)
}

fn red(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> bool {
    let c = fb.get_pixel(x, y);
    c.r > 0.9 && c.g < 0.1 && c.b < 0.1
}

#[test]
fn an_authored_position_and_size_rect_is_drawn_on_every_frame() {
    let (mut tree, _) = boot();
    for n in 1..=3 {
        let fb = frame(&mut tree);
        assert!(red(&fb, 15, 15), "frame {n}: the Plate is drawn where it was authored");
        assert!(!red(&fb, 25, 15), "frame {n}: and only there");
    }
}

#[test]
fn a_position_assigned_later_moves_the_control_and_it_stays_moved() {
    let (mut tree, main) = boot();
    frame(&mut tree);
    let plate = tree.get_node_relative(main, "Plate").unwrap();
    tree.get_node_mut(plate)
        .unwrap()
        .set_property("position", Variant::Vector2(Vector2::new(50.0, 30.0)));
    for n in 1..=2 {
        let fb = frame(&mut tree);
        assert!(red(&fb, 55, 35), "frame {n} after the move: drawn at the new position");
        assert!(!red(&fb, 15, 15), "frame {n} after the move: gone from the old one");
    }
}

#[test]
fn an_anchored_control_still_follows_its_anchors() {
    let (mut tree, main) = boot();
    let fb = frame(&mut tree);
    let c = fb.get_pixel(95, 45);
    assert!(c.g > 0.9 && c.r < 0.1, "anchored right: x 90..100 of a 100-wide view");
    let anchored = tree.get_node_relative(main, "Anchored").unwrap();
    let pos = tree.get_node(anchored).unwrap().get_property("position");
    assert_eq!(pos, Variant::Vector2(Vector2::new(90.0, 40.0)));
}
