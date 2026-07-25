//! pat-0q2kx.1 acceptance: an `AnimationPlayer` authored in a `.tscn` actually
//! animates when the scene is loaded through the normal runner boot path.
//!
//! Patina already had the whole sampling engine (`gdscene::animation`) and the
//! tree hook (`SceneTree::attach_animation_player`), but **nothing built a
//! player from a loaded scene** — `attach_animation_player` had zero non-test
//! callers, `gdresource` had no parser for an Animation sub-resource's
//! `tracks/N/*` keys, and the queued playback signals were never re-emitted as
//! node signals. So a Godot project whose `.tscn` carries an AnimationPlayer
//! ran with the sprite frozen and `animation_finished` never firing.
//!
//! These tests deliberately construct **nothing** by hand: the animation, the
//! library, the player, the autoplay clip and the signal connection all come
//! out of the `.tscn`, and playback is driven by real `MainLoop::step` frames
//! through `EngineBootstrap`. There is no `attach_animation_player` call in
//! this file — if the boot wiring regresses, these tests fail.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use gdvariant::Variant;
use patina_runner::bootstrap::{BootConfig, EngineBootstrap};

/// `boot_project` mutates the process-global `res://` path roots, so serialize
/// these tests to stay correct under `cargo test` (one process, many threads).
/// `cargo nextest` isolates each test in its own process and doesn't need this.
static BOOT_LOCK: Mutex<()> = Mutex::new(());

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_0q2kx1_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A scene exactly as the Godot editor writes it: an `[sub_resource
/// type="Animation"]` with a multi-line `tracks/0/keys` dictionary, gathered by
/// an `[sub_resource type="AnimationLibrary"]`, referenced from the player's
/// `libraries`, with `autoplay` and a `[connection]` to the root's script.
const MAIN_TSCN: &str = r#"[gd_scene load_steps=4 format=3]

[ext_resource type="Script" path="res://main.gd" id="1_main"]

[sub_resource type="Animation" id="Animation_slide"]
resource_name = "slide"
length = 1.0
loop_mode = 0
tracks/0/type = "value"
tracks/0/imported = false
tracks/0/enabled = true
tracks/0/path = NodePath("Sprite2D:position")
tracks/0/interp = 1
tracks/0/loop_wrap = true
tracks/0/keys = {
"times": PackedFloat32Array(0, 1),
"transitions": PackedFloat32Array(1, 1),
"update": 0,
"values": [Vector2(0, 0), Vector2(100, 0)]
}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {
"slide": SubResource("Animation_slide")
}

[node name="Main" type="Node2D"]
script = ExtResource("1_main")

[node name="Sprite2D" type="Sprite2D" parent="."]
position = Vector2(0, 0)

[node name="AnimationPlayer" type="AnimationPlayer" parent="."]
libraries = {
"": SubResource("AnimationLibrary_main")
}
autoplay = "slide"

[connection signal="animation_finished" from="AnimationPlayer" to="." method="_on_animation_finished"]
"#;

const MAIN_GD: &str = "\
extends Node2D
var finished_clip = \"\"
var finished_count = 0

func _on_animation_finished(anim_name):
    finished_clip = anim_name
    finished_count += 1
";

fn write_project(dir: &Path, tscn: &str, gd: &str) {
    std::fs::write(
        dir.join("project.godot"),
        "[application]\nconfig/name=\"AnimPlayerBoot\"\nrun/main_scene=\"res://main.tscn\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.tscn"), tscn).unwrap();
    std::fs::write(dir.join("main.gd"), gd).unwrap();
}

fn boot(dir: &Path) -> EngineBootstrap {
    let config = BootConfig {
        main_scene: Some(dir.join("main.tscn")),
        project_dir: dir.to_path_buf(),
        headless: true,
        ..BootConfig::default()
    };
    let mut boot = EngineBootstrap::new(config);
    boot.run_all().expect("boot succeeds");
    boot
}

/// Reads a script variable's current value from a node.
fn script_var(
    tree: &gdscene::SceneTree,
    node_id: gdscene::node::NodeId,
    var: &str,
) -> Option<Variant> {
    tree.get_script(node_id)?
        .list_properties()
        .into_iter()
        .find(|p| p.name == var)
        .map(|p| p.default_value)
}

fn sprite_x(tree: &gdscene::SceneTree) -> f32 {
    let sprite = tree
        .get_node_by_path("/root/Main/Sprite2D")
        .expect("Sprite2D instanced");
    match tree.get_node(sprite).unwrap().get_property("position") {
        Variant::Vector2(v) => v.x,
        other => panic!("Sprite2D.position should be a Vector2, got {other:?}"),
    }
}

/// **The acceptance test.** A `.tscn` AnimationPlayer with an autoplay clip
/// moves a `Sprite2D`'s `position` across `MainLoop::step` frames, and the
/// GDScript connected to `animation_finished` observes the signal — all from
/// the scene file, with no test-side `attach_animation_player`.
#[test]
fn animation_player_tscn_autoplay_drives_property() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("autoplay");
    write_project(&dir, MAIN_TSCN, MAIN_GD);

    let mut boot = boot(&dir);
    let main_id = boot
        .main_loop()
        .expect("main loop built")
        .tree()
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    // The scene load — not the test — attached a player and armed the autoplay
    // clip. This is the wiring that did not exist before pat-0q2kx.1.
    {
        let tree = boot.main_loop().unwrap().tree();
        let player_node = tree
            .get_node_by_path("/root/Main/AnimationPlayer")
            .expect("AnimationPlayer node instanced");
        let player = tree
            .get_animation_player(player_node)
            .expect("scene load attached an AnimationPlayer");
        assert!(
            player.animations.contains_key("slide"),
            "the .tscn AnimationLibrary clip was parsed into the player, got {:?}",
            player.animations.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            player.current_animation(),
            Some("slide"),
            "autoplay started the declared clip when the scene entered the tree"
        );
    }

    // Sprite starts at the track's first keyframe.
    assert_eq!(sprite_x(boot.main_loop().unwrap().tree()), 0.0);

    // Half the 1.0s animation: 5 frames of 0.1s.
    for _ in 0..5 {
        boot.main_loop_mut().unwrap().step(0.1);
    }
    let mid_x = sprite_x(boot.main_loop().unwrap().tree());
    assert!(
        (mid_x - 50.0).abs() < 1.0,
        "half-way through the clip the sprite has moved to ~50, got {mid_x}"
    );

    // The signal has not fired yet — the clip has not finished.
    assert_eq!(
        script_var(boot.main_loop().unwrap().tree(), main_id, "finished_count"),
        Some(Variant::Int(0)),
        "animation_finished must not fire before the clip ends"
    );

    // Run past the end of the clip.
    for _ in 0..6 {
        boot.main_loop_mut().unwrap().step(0.1);
    }

    let end_x = sprite_x(boot.main_loop().unwrap().tree());
    assert!(
        (end_x - 100.0).abs() < 0.01,
        "at the end of the clip the sprite sits on the last keyframe (100), got {end_x}"
    );

    let tree = boot.main_loop().unwrap().tree();
    assert_eq!(
        script_var(tree, main_id, "finished_clip"),
        Some(Variant::String("slide".into())),
        "the GDScript connected to animation_finished ran with the clip name"
    );
    assert_eq!(
        script_var(tree, main_id, "finished_count"),
        Some(Variant::Int(1)),
        "animation_finished fires exactly once for a non-looping clip"
    );
}

/// A looping clip keeps playing and never emits `animation_finished`, so the
/// signal re-emission cannot degrade into "fire every frame".
#[test]
fn looping_tscn_animation_does_not_emit_finished() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("looping");
    write_project(&dir, &MAIN_TSCN.replace("loop_mode = 0", "loop_mode = 1"), MAIN_GD);

    let mut boot = boot(&dir);
    let main_id = boot
        .main_loop()
        .expect("main loop built")
        .tree()
        .get_node_by_path("/root/Main")
        .expect("main scene instanced");

    // Three full loops.
    for _ in 0..30 {
        boot.main_loop_mut().unwrap().step(0.1);
    }

    let tree = boot.main_loop().unwrap().tree();
    assert_eq!(
        script_var(tree, main_id, "finished_count"),
        Some(Variant::Int(0)),
        "a looping clip never finishes, so animation_finished never fires"
    );
    let player_node = tree.get_node_by_path("/root/Main/AnimationPlayer").unwrap();
    assert!(
        tree.get_animation_player(player_node).unwrap().playing,
        "a looping clip is still playing after three loops"
    );
}

/// `$AnimationPlayer.play("clip")` from GDScript starts that clip rather than
/// being swallowed by the AudioStreamPlayer `playing = true` semantics, which
/// is what the shared `ObjectId.play` dispatch did before pat-0q2kx.1.
#[test]
fn gdscript_play_selects_the_named_clip() {
    let _guard = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_project_dir("play");

    // Two clips; autoplay picks "slide", the script switches to "lift".
    let tscn = MAIN_TSCN
        .replace("load_steps=4", "load_steps=5")
        .replace(
            "[sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_main\"]\n_data = {\n\"slide\": SubResource(\"Animation_slide\")\n}",
            "[sub_resource type=\"Animation\" id=\"Animation_lift\"]\n\
             resource_name = \"lift\"\n\
             length = 1.0\n\
             loop_mode = 0\n\
             tracks/0/type = \"value\"\n\
             tracks/0/path = NodePath(\"Sprite2D:position\")\n\
             tracks/0/interp = 1\n\
             tracks/0/keys = {\n\
             \"times\": PackedFloat32Array(0, 1),\n\
             \"transitions\": PackedFloat32Array(1, 1),\n\
             \"update\": 0,\n\
             \"values\": [Vector2(0, 0), Vector2(0, -80)]\n\
             }\n\n\
             [sub_resource type=\"AnimationLibrary\" id=\"AnimationLibrary_main\"]\n\
             _data = {\n\
             \"lift\": SubResource(\"Animation_lift\"),\n\
             \"slide\": SubResource(\"Animation_slide\")\n\
             }",
        );
    let gd = "\
extends Node2D
var finished_clip = \"\"
var finished_count = 0

func _ready():
    $AnimationPlayer.play(\"lift\")

func _on_animation_finished(anim_name):
    finished_clip = anim_name
    finished_count += 1
";
    write_project(&dir, &tscn, gd);

    let mut boot = boot(&dir);

    // `_ready` ran during boot and switched the clip away from the autoplay one.
    {
        let tree = boot.main_loop().unwrap().tree();
        let player_node = tree.get_node_by_path("/root/Main/AnimationPlayer").unwrap();
        assert_eq!(
            tree.get_animation_player(player_node)
                .expect("player attached")
                .current_animation(),
            Some("lift"),
            "play(\"lift\") selected the named clip instead of setting `playing = true`"
        );
    }

    // The "lift" track drives y, not x — proof the right clip is sampled.
    for _ in 0..5 {
        boot.main_loop_mut().unwrap().step(0.1);
    }
    let sprite = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node_by_path("/root/Main/Sprite2D")
        .unwrap();
    let pos = boot
        .main_loop()
        .unwrap()
        .tree()
        .get_node(sprite)
        .unwrap()
        .get_property("position");
    match pos {
        Variant::Vector2(v) => {
            assert!(
                (v.y + 40.0).abs() < 1.0 && v.x.abs() < 1.0,
                "the 'lift' clip animates y toward -80, got {v:?}"
            );
        }
        other => panic!("expected Vector2 position, got {other:?}"),
    }
}
