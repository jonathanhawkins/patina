//! An anchored Control is clickable whether or not a frame has been rendered.
//!
//! Found playtesting the official Godot demo "Dodge the Creeps" through the
//! agent interface's frame-exact `play`, which steps the game without drawing:
//! its Start button is placed purely by anchors and offsets, and a Control's
//! rect was resolved only by the RENDER pass's layout — so before any render
//! (or after the window changed size) pointer hit-testing used no rect, or a
//! stale one, and the click missed. Godot lays Controls out in the scene tree.

use gdcore::math::Vector2;
use gdplatform::input::{InputEvent, MouseButton};
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop, NodeId, SceneTree};
use gdvariant::Variant;

/// Dodge the Creeps' StartButton: bottom-centre anchored, 180x100.
const HUD: &str = r#"[gd_scene format=3]

[node name="HUD" type="CanvasLayer"]

[node name="StartButton" type="Button" parent="."]
anchors_preset = 7
anchor_left = 0.5
anchor_top = 1.0
anchor_right = 0.5
anchor_bottom = 1.0
offset_left = -90.0
offset_top = -200.0
offset_right = 90.0
offset_bottom = -100.0
grow_horizontal = 2
grow_vertical = 0
text = "Start"

[connection signal="pressed" from="StartButton" to="." method="_on_start"]
"#;

const COUNTER_GD: &str = "\
extends CanvasLayer
var starts = 0
func _on_start():
    self.starts = self.starts + 1
";

fn boot() -> (MainLoop, NodeId) {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(HUD).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let hud = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    tree.attach_script(
        hud,
        Box::new(GDScriptNodeInstance::from_source(COUNTER_GD, hud).unwrap()),
    );
    LifecycleManager::enter_tree(&mut tree, hud);
    let mut ml = MainLoop::new(tree);
    // Dodge's 480x720 window. Nothing is ever rendered in this test.
    ml.set_viewport_size(480, 720);
    ml.step(1.0 / 60.0);
    (ml, hud)
}

fn click(ml: &mut MainLoop, x: f32, y: f32) {
    let position = Vector2::new(x, y);
    ml.push_event(InputEvent::MouseMotion {
        position,
        relative: Vector2::ZERO,
    });
    for pressed in [true, false] {
        ml.push_event(InputEvent::MouseButton {
            button: MouseButton::Left,
            pressed,
            position,
        });
        ml.step(1.0 / 60.0);
    }
}

fn starts(ml: &MainLoop, hud: NodeId) -> Variant {
    ml.tree()
        .get_script(hud)
        .and_then(|s| s.get_property("starts"))
        .unwrap_or(Variant::Nil)
}

#[test]
fn an_anchored_button_is_clicked_with_no_frame_ever_rendered() {
    let (mut ml, hud) = boot();
    // Outside the button (above it): nothing.
    click(&mut ml, 240.0, 400.0);
    assert_eq!(starts(&ml, hud), Variant::Int(0), "a click above the button misses");
    // The button's anchored rect is x 150..330, y 520..620.
    click(&mut ml, 240.0, 570.0);
    assert_eq!(
        starts(&ml, hud),
        Variant::Int(1),
        "a click at the anchored button's centre presses it, render or not"
    );
}
