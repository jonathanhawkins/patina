//! Found playing the official Godot demo "Pong": a 640x400 game with
//! `stretch/aspect="expand"` in a taller window grows its viewport to 640x542,
//! and its Camera2D (`offset = (320, 200)`) centres the playfield in it, with
//! the project's `default_clear_color` above and below. The Play frame instead
//! showed the playfield flush at the top and a GREY band below:
//!
//! - `current_camera2d` read an ABSENT `enabled` key as "disabled". Godot's
//!   default is `true` and a `.tscn` never writes it, so on an un-seeded node
//!   the camera was ignored and the canvas sat at the origin.
//! - `render_play_frame` cleared to a fixed grey instead of the project's
//!   `rendering/environment/defaults/default_clear_color`.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;

const PROJECT: &str = r#"config_version=5

[display]

window/size/viewport_width=640
window/size/viewport_height=400
window/stretch/mode="canvas_items"
window/stretch/aspect="expand"
window/stretch/scale_mode="integer"

[rendering]

environment/defaults/default_clear_color=Color(0.1, 0.1, 0.5, 1)
"#;

const SCENE: &str = r#"[gd_scene format=3]

[node name="Pong" type="Node2D"]

[node name="Background" type="ColorRect" parent="."]
offset_right = 640.0
offset_bottom = 400.0
color = Color(0, 1, 0, 1)

[node name="Camera2D" type="Camera2D" parent="."]
offset = Vector2(320, 200)
"#;

#[test]
fn the_camera_centres_the_playfield_and_the_margin_is_the_project_clear_colour() {
    // ⚠️ No `register_all_classes()`: the un-seeded node is the case that broke.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.godot"), PROJECT).unwrap();
    let settings =
        gdcore::ProjectSettings::load_godot_project(&dir.path().join("project.godot")).unwrap();

    let mut tree = SceneTree::new();
    tree.root_window_mut().config = settings.content_scale_config();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &PackedScene::from_tscn(SCENE).unwrap()).unwrap();

    // 995x843 window -> 640x542 viewport at integer scale 1, margins 178/151.
    // Camera centre (320,200) puts canvas y=0 at viewport y=71 (window 222).
    let mut cache = Default::default();
    let fb = gdscene::canvas_render::render_play_frame(&mut tree, 995, 843, &mut cache, &settings);
    let at = |y: u32| {
        let c = fb.get_pixel(498, y);
        ((c.r * 10.0).round(), (c.g * 10.0).round(), (c.b * 10.0).round())
    };
    let clear = (1.0, 1.0, 5.0);
    let green = (0.0, 10.0, 0.0);
    assert_eq!(at(181), clear, "above the playfield: the project's clear colour (camera centred)");
    assert_eq!(at(251), green, "the playfield starts ~71 px down, not at the top");
    assert_eq!(at(600), green, "and is 400 tall");
    assert_eq!(at(660), clear, "below it, the clear colour again — not Godot's default grey");
}
