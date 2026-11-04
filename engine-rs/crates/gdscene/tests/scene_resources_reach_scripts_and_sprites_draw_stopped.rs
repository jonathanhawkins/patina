//! Two runtime gaps found playing the official Godot demo "Dodge the Creeps":
//!
//! 1. `main.gd` declares `@export var mob_scene: PackedScene` and `main.tscn`
//!    sets it with `mob_scene = ExtResource("2_50pww")`. The script received
//!    the raw reference TEXT, so `mob_scene.instantiate()` raised
//!    "String.instantiate" on every spawn and no enemy ever appeared.
//! 2. The player's `AnimatedSprite2D` (`animation = &"right"`, stopped by
//!    `player.gd` whenever it stands still) drew NOTHING: a sprite that is not
//!    playing never published its frame texture, and the StringName
//!    `animation` was not read at all.
//!
//! Both run through a real `res://` project, the scene boot a runtime uses,
//! and a stepped `MainLoop`.

use std::path::PathBuf;
use std::sync::Mutex;

use gdscene::packed_scene::{add_packed_scene_to_tree_from_res, PackedScene};
use gdscene::scripting::attach_subtree_scripts_from_res;
use gdscene::{LifecycleManager, MainLoop, NodeId, SceneTree};
use gdvariant::Variant;

static ROOTS: Mutex<()> = Mutex::new(());

const MOB: &str = r#"[gd_scene format=3]

[node name="Mob" type="RigidBody2D"]
"#;

const MAIN_GD: &str = "\
extends Node
@export var mob_scene: PackedScene
var spawned = \"\"
var scene_class = \"\"
func _ready():
    self.scene_class = mob_scene.get_class()
    var mob = mob_scene.instantiate()
    add_child(mob)
    self.spawned = mob.name
";

const MAIN: &str = r#"[gd_scene format=3]

[ext_resource type="Script" path="res://main.gd" id="1_s"]
[ext_resource type="PackedScene" path="res://mob.tscn" id="2_m"]

[node name="Main" type="Node"]
script = ExtResource("1_s")
mob_scene = ExtResource("2_m")
"#;

fn project(files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "patina-scene-resources-{}-{}",
        std::process::id(),
        files.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    dir
}

fn boot(tscn: &str) -> (MainLoop, NodeId) {
    let scene = PackedScene::from_tscn(tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree_from_res(&mut tree, root, &scene).unwrap();
    attach_subtree_scripts_from_res(&mut tree, main);
    LifecycleManager::enter_tree(&mut tree, main);
    (MainLoop::new(tree), main)
}

#[test]
fn an_exported_packed_scene_set_in_the_tscn_reaches_the_script_as_a_scene() {
    let _guard = ROOTS.lock().unwrap_or_else(|p| p.into_inner());
    gdobject::class_db::register_all_classes();
    let dir = project(&[("mob.tscn", MOB.as_bytes()), ("main.gd", MAIN_GD.as_bytes())]);
    gdcore::file_access::set_path_roots(Some(dir), None);

    let (ml, main) = boot(MAIN);
    let script = ml.tree().get_script(main).expect("main.gd attached");
    assert_eq!(
        script.get_property("scene_class"),
        Some(Variant::String("PackedScene".into())),
        "the export must hold the loaded PackedScene, not the reference text"
    );
    assert_eq!(
        script.get_property("spawned"),
        Some(Variant::String("Mob".into())),
        "mob_scene.instantiate() must build the referenced scene"
    );
    assert!(
        ml.tree().get_node_relative(main, "Mob").is_some(),
        "the instance was added under Main"
    );
}

/// Placeholder bytes: the assertions are about WHICH frame's texture path the
/// sprite publishes, which never decodes the file.
fn png() -> Vec<u8> {
    b"not decoded".to_vec()
}

/// The player's sprite, as Dodge the Creeps writes it: two clips, the scene
/// selects `&"right"` (a StringName), and nothing ever calls `play()`.
const SPRITE: &str = r#"[gd_scene format=3]

[ext_resource type="Texture2D" path="res://walk1.png" id="1"]
[ext_resource type="Texture2D" path="res://walk2.png" id="2"]
[ext_resource type="Texture2D" path="res://up1.png" id="3"]

[sub_resource type="SpriteFrames" id="1"]
animations = [{
"frames": [{
"duration": 1.0,
"texture": ExtResource("1")
}, {
"duration": 1.0,
"texture": ExtResource("2")
}],
"loop": true,
"name": &"right",
"speed": 5.0
}, {
"frames": [{
"duration": 1.0,
"texture": ExtResource("3")
}],
"loop": true,
"name": &"up",
"speed": 5.0
}]

[node name="Player" type="Node2D"]

[node name="AnimatedSprite2D" type="AnimatedSprite2D" parent="."]
sprite_frames = SubResource("1")
animation = &"right"
"#;

#[test]
fn a_stopped_animated_sprite_draws_its_current_frame_of_a_stringname_animation() {
    let _guard = ROOTS.lock().unwrap_or_else(|p| p.into_inner());
    gdobject::class_db::register_all_classes();
    let bytes = png();
    let dir = project(&[
        ("walk1.png", &bytes),
        ("walk2.png", &bytes),
        ("up1.png", &bytes),
    ]);
    gdcore::file_access::set_path_roots(Some(dir), None);

    let (mut ml, main) = boot(SPRITE);
    let sprite = ml.tree().get_node_relative(main, "AnimatedSprite2D").unwrap();
    ml.step(1.0 / 60.0);
    let texture = |ml: &MainLoop| ml.tree().get_node(sprite).unwrap().get_property("texture");
    assert_eq!(
        texture(&ml),
        Variant::String("res://walk1.png".into()),
        "a never-played sprite draws frame 0 of the scene's `&\"right\"` clip"
    );

    // Standing still: frame 1 selected, still not playing — that frame draws,
    // and it does not advance.
    ml.tree_mut()
        .get_node_mut(sprite)
        .unwrap()
        .set_property("frame", Variant::Int(1));
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(texture(&ml), Variant::String("res://walk2.png".into()));
    assert_eq!(
        ml.tree().get_node(sprite).unwrap().get_property("frame"),
        Variant::Int(1),
        "a stopped sprite holds its frame"
    );

    // A script switching the clip by StringName, still stopped.
    {
        let node = ml.tree_mut().get_node_mut(sprite).unwrap();
        node.set_property(
            "animation",
            Variant::StringName(gdcore::string_name::StringName::new("up")),
        );
        node.set_property("frame", Variant::Int(0));
    }
    ml.step(1.0 / 60.0);
    assert_eq!(texture(&ml), Variant::String("res://up1.png".into()));
}
