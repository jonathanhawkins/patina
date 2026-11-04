//! Controls and Node2Ds draw in ONE canvas order — layer, then z_index, then
//! tree order — as every CanvasItem does in Godot.
//!
//! Found playing the official Godot demo "Dodge the Creeps": `main.tscn`'s first
//! child is a full-rect `ColorRect` background, and the player and enemies are
//! Node2Ds after it. The themed Control pass gave every Control primitive
//! `CONTROL_Z_BASE + walk order` on canvas layer 0, i.e. "above every Node2D",
//! so the opaque background covered the player and every enemy: the game ran
//! with nothing visible but the HUD. The ColorRect was also drawn TWICE (once
//! by the runtime list, once by the Control pass), double-blending any
//! translucent colour.
//!
//! Driven through `render_play_frame`, the editor Play's frame entry point.

use gdscene::canvas_render::render_play_frame;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::texture_cache::TextureCache;
use gdscene::{LifecycleManager, SceneTree};

const SCENE: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Background" type="ColorRect" parent="."]
anchors_preset = 15
anchor_right = 1.0
anchor_bottom = 1.0
color = Color(0, 0, 1, 1)

[node name="Enemy" type="Polygon2D" parent="."]
color = Color(1, 0, 0, 1)
polygon = PackedVector2Array(10, 10, 40, 10, 40, 40, 10, 40)

[node name="Tint" type="ColorRect" parent="."]
offset_left = 60.0
offset_top = 10.0
offset_right = 90.0
offset_bottom = 40.0
color = Color(1, 1, 1, 0.5)

[node name="HUD" type="CanvasLayer" parent="."]

[node name="Banner" type="ColorRect" parent="HUD"]
offset_left = 20.0
offset_top = 20.0
offset_right = 30.0
offset_bottom = 30.0
color = Color(0, 1, 0, 1)
"#;

fn render() -> gdrender2d::renderer::FrameBuffer {
    render_scene(SCENE)
}

fn render_scene(source: &str) -> gdrender2d::renderer::FrameBuffer {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(source).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    LifecycleManager::enter_tree(&mut tree, main);
    let mut cache = TextureCache::new(".");
    let settings = gdcore::project_settings::ProjectSettings::default();
    // ⚠️ The SECOND frame: the runtime list sizes a ColorRect from the `size`
    // layout writes back, so on the first frame its duplicate is zero-sized and
    // the double blend only shows from frame 2 on — as in a running game.
    render_play_frame(&mut tree, 100, 60, &mut cache, &settings);
    render_play_frame(&mut tree, 100, 60, &mut cache, &settings)
}

fn rgb(fb: &gdrender2d::renderer::FrameBuffer, x: u32, y: u32) -> [u8; 3] {
    let c = fb.get_pixel(x, y);
    let q = |v: f32| (v * 255.0).round() as u8;
    [q(c.r), q(c.g), q(c.b)]
}

#[test]
fn a_node2d_after_a_background_colorrect_draws_on_top_of_it() {
    let fb = render();
    // Control region: background only.
    assert_eq!(rgb(&fb, 95, 55), [0, 0, 255], "the background fills the view");
    // The polygon is later in the tree at the same z: it draws over the
    // background (it used to be covered by it).
    assert_eq!(rgb(&fb, 12, 12), [255, 0, 0], "the Node2D after the ColorRect is visible");
}

/// Only a translucent ColorRect over the clear colour: with nothing opaque in
/// between, a second copy of it would show.
const TRANSLUCENT_ONLY: &str = r#"[gd_scene format=3]

[node name="Main" type="Node"]

[node name="Tint" type="ColorRect" parent="."]
offset_left = 60.0
offset_top = 10.0
offset_right = 90.0
offset_bottom = 40.0
color = Color(1, 1, 1, 0.5)
"#;

#[test]
fn a_translucent_colorrect_blends_once() {
    let fb = render_scene(TRANSLUCENT_ONLY);
    let clear = gdscene::canvas_render::RUNTIME_CLEAR_COLOR;
    let once = |c: f32| ((0.5 + 0.5 * c) * 255.0).round() as i32;
    let [r, g, b] = rgb(&fb, 75, 25);
    for (got, want) in [(r, once(clear.r)), (g, once(clear.g)), (b, once(clear.b))] {
        assert!(
            (got as i32 - want).abs() <= 1,
            "half-white blended ONCE over the clear colour: want ~{want}, got {:?}",
            [r, g, b]
        );
    }
}

#[test]
fn a_control_in_a_canvas_layer_still_draws_above_the_default_canvas() {
    let fb = render();
    // The HUD banner overlaps the polygon and must win: canvas layer 1 > 0.
    assert_eq!(rgb(&fb, 25, 25), [0, 255, 0]);
    // ...and only inside its own rect.
    assert_eq!(rgb(&fb, 35, 35), [255, 0, 0]);
}
