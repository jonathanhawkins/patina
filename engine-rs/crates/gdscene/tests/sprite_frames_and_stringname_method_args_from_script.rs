//! Script idioms from the official Godot demo "Dodge the Creeps", each a
//! runtime error in Patina when the game was imported:
//!
//! - `mob.gd`: `Array($AnimatedSprite2D.sprite_frames.get_animation_names())`
//!   then `.pick_random()` — `sprite_frames` read back as the text
//!   `"SubResource:1"`, so `get_animation_names` was "method not found on
//!   String" and every mob died in `_ready`;
//! - `player.gd`: `$CollisionShape2D.set_deferred(&"disabled", true)` — a
//!   StringName property name was rejected ("set_deferred() takes (property,
//!   value)"), so a hit never disabled the collider.
//!
//! Each runs through a real scene and a stepped `MainLoop`.

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::{GDScriptNodeInstance, LifecycleManager, MainLoop, NodeId, SceneTree};
use gdvariant::Variant;

const MOB: &str = r#"[gd_scene format=3]

[sub_resource type="SpriteFrames" id="1"]
animations = [{
"frames": [],
"loop": true,
"name": &"fly",
"speed": 3.0
}, {
"frames": [],
"loop": false,
"name": &"swim",
"speed": 4.0
}, {
"frames": [],
"loop": true,
"name": &"walk",
"speed": 4.0
}]

[sub_resource type="RectangleShape2D" id="2"]

[node name="Mob" type="Node2D"]

[node name="AnimatedSprite2D" type="AnimatedSprite2D" parent="."]
sprite_frames = SubResource("1")
animation = &"walk"

[node name="CollisionShape2D" type="CollisionShape2D" parent="."]
shape = SubResource("2")
"#;

const MOB_GD: &str = "\
extends Node2D
var names = []
var picked = \"\"
var has_swim = false
var has_run = true
var swim_speed = 0.0
var swim_loops = true
var walk_frames = -1
var disabled_now = true
var disabled_later = false
func _ready():
    var mob_types = Array($AnimatedSprite2D.sprite_frames.get_animation_names())
    self.names = mob_types
    $AnimatedSprite2D.animation = mob_types.pick_random()
    self.picked = $AnimatedSprite2D.animation
    var frames = $AnimatedSprite2D.sprite_frames
    self.has_swim = frames.has_animation(&\"swim\")
    self.has_run = frames.has_animation(\"run\")
    self.swim_speed = frames.get_animation_speed(&\"swim\")
    self.swim_loops = frames.get_animation_loop(&\"swim\")
    self.walk_frames = frames.get_frame_count(&\"walk\")
    $CollisionShape2D.set_deferred(&\"disabled\", true)
    self.disabled_now = $CollisionShape2D.disabled
func _process(delta):
    self.disabled_later = $CollisionShape2D.disabled
";

fn var(ml: &MainLoop, id: NodeId, name: &str) -> Variant {
    ml.tree()
        .get_script(id)
        .and_then(|s| s.get_property(name))
        .unwrap_or(Variant::Nil)
}

#[test]
fn the_mob_spawn_idioms_run() {
    gdobject::class_db::register_all_classes();
    let scene = PackedScene::from_tscn(MOB).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let mob = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    tree.attach_script(
        mob,
        Box::new(GDScriptNodeInstance::from_source(MOB_GD, mob).expect("mob.gd parses")),
    );
    LifecycleManager::enter_tree(&mut tree, mob);
    let mut ml = MainLoop::new(tree);
    ml.step(1.0 / 60.0);

    let names = match var(&ml, mob, "names") {
        Variant::Array(a) => a
            .to_vec()
            .into_iter()
            .map(|v| match v {
                Variant::String(s) => s,
                Variant::StringName(s) => s.as_str().to_string(),
                other => panic!("a clip name should be a string, got {other:?}"),
            })
            .collect::<Vec<_>>(),
        other => panic!("`names` should be the clip list, got {other:?}"),
    };
    assert_eq!(
        names,
        ["fly", "swim", "walk"],
        "in the resource's own order"
    );

    let picked = match var(&ml, mob, "picked") {
        Variant::String(s) => s,
        Variant::StringName(s) => s.as_str().to_string(),
        other => panic!("`picked` should be a clip name, got {other:?}"),
    };
    assert!(
        names.contains(&picked),
        "pick_random chose one of them: {picked}"
    );

    assert_eq!(var(&ml, mob, "has_swim"), Variant::Bool(true));
    assert_eq!(var(&ml, mob, "has_run"), Variant::Bool(false));
    assert_eq!(var(&ml, mob, "swim_speed"), Variant::Float(4.0));
    assert_eq!(var(&ml, mob, "swim_loops"), Variant::Bool(false));
    assert_eq!(var(&ml, mob, "walk_frames"), Variant::Int(0));

    // `set_deferred` with a StringName name: not yet applied inside `_ready`,
    // applied by the next frame.
    assert_ne!(var(&ml, mob, "disabled_now"), Variant::Bool(true));
    ml.step(1.0 / 60.0);
    assert_eq!(
        var(&ml, mob, "disabled_later"),
        Variant::Bool(true),
        "set_deferred(&\"disabled\", true) must apply"
    );
}
