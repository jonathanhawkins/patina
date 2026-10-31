//! pat-mgixk.1 acceptance: an `AnimationPlayer` in a scene saved by Godot 4.6
//! actually animates.
//!
//! Godot 4.6 stores each animation library as its own storage property,
//! `libraries/<name>` — `AnimationMixer::_get_property_list` publishes
//! `libraries/%s` with `PROPERTY_USAGE_STORAGE`, and `_set` only reads
//! `libraries = {..}` inside the `!DISABLE_DEPRECATED` compat branch
//! (upstream/godot/scene/animation/animation_mixer.cpp). The default library's
//! name is empty, so a 4.6 scene writes the key as literally `libraries/`.
//!
//! Patina read ONLY the deprecated dictionary (`animation_tscn.rs`
//! `build_animation_player`), so an `AnimationPlayer` in any scene the current
//! editor saves resolved no library at all, `build_animation_player` returned
//! `None`, nothing was attached, and every `play()` on that node fell through
//! to setting a bare `playing` property. Two further gaps sat behind it: a
//! library referenced as an `ExtResource` (a `.tres` beside the scene) was
//! dropped because only `SubResource:` ids resolved, and `gdresource`'s `.tres`
//! loader could not read a library file at all — Godot's text writer puts every
//! Dictionary on several lines, and the loader rejected the bare `{`, failing
//! the whole file. It also refused `&"clip"` dictionary keys, which is exactly
//! how `AnimationLibrary::_get_data` saves them (it keys `animations` by
//! `StringName`).
//!
//! This drives the real chain, constructing nothing by hand:
//!
//! ```text
//! .tscn (+ .tres) -> PackedScene::from_tscn -> add_packed_scene_to_tree
//!   -> attach_scene_animation_players -> MainLoop::step
//!     -> SceneTree::process_animations -> the node's `position`
//! ```
//!
//! There is no `attach_animation_player` and no `add_animation` call here: if
//! the scene-load wiring stops resolving a 4.6 library, the `Mover` never moves
//! and this fails.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_tree::SceneTree;
use gdscene::MainLoop;
use gdvariant::Variant;

/// The `[sub_resource type="Animation"]` body Godot writes: a one-second value
/// track moving `Mover.position` from x=0 to x=100, with the multi-line
/// `tracks/0/keys` dictionary the editor emits.
const ANIMATION_BODY: &str = r#"resource_name = "slide"
length = 1.0
loop_mode = 0
tracks/0/type = "value"
tracks/0/imported = false
tracks/0/enabled = true
tracks/0/path = NodePath("Mover:position")
tracks/0/interp = 1
tracks/0/loop_wrap = true
tracks/0/keys = {
"times": PackedFloat32Array(0, 1),
"transitions": PackedFloat32Array(1, 1),
"update": 0,
"values": [Vector2(0, 0), Vector2(100, 0)]
}"#;

/// A 4.6 scene whose library is an inline `[sub_resource]`, referenced through
/// the `libraries/` storage property rather than the deprecated dictionary.
///
/// ⚠️ `_data`'s key is `&"slide"`: `AnimationLibrary::_get_data` returns a
/// Dictionary keyed by `StringName`, so that is what the text writer emits.
fn sub_resource_scene() -> String {
    format!(
        r#"[gd_scene load_steps=3 format=3]

[sub_resource type="Animation" id="Animation_slide"]
{ANIMATION_BODY}

[sub_resource type="AnimationLibrary" id="AnimationLibrary_main"]
_data = {{
&"slide": SubResource("Animation_slide")
}}

[node name="World" type="Node2D"]

[node name="Mover" type="Node2D" parent="."]
position = Vector2(0, 0)

[node name="Anim" type="AnimationPlayer" parent="."]
libraries/ = SubResource("AnimationLibrary_main")
autoplay = "slide"
"#
    )
}

/// The same library as a standalone `AnimationLibrary` resource file, exactly
/// as Godot saves one: a `[gd_resource type="AnimationLibrary"]` header, the
/// Animation as its own `[sub_resource]`, and a multi-line `_data` in
/// `[resource]`.
fn library_tres() -> String {
    format!(
        r#"[gd_resource type="AnimationLibrary" load_steps=2 format=3]

[sub_resource type="Animation" id="Animation_slide"]
{ANIMATION_BODY}

[resource]
_data = {{
&"slide": SubResource("Animation_slide")
}}
"#
    )
}

/// A 4.6 scene whose library lives in the `.tres` beside it.
const EXT_RESOURCE_SCENE: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="AnimationLibrary" path="res://lib.tres" id="1_lib"]

[node name="World" type="Node2D"]

[node name="Mover" type="Node2D" parent="."]
position = Vector2(0, 0)

[node name="Anim" type="AnimationPlayer" parent="."]
libraries/ = ExtResource("1_lib")
autoplay = "slide"
"#;

fn unique_project_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_mgixk1_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the scratch project dir");
    dir
}

/// Instances `tscn` the way a game does and hands back the loop driving it.
///
/// `add_packed_scene_to_tree` is what calls `attach_scene_animation_players`
/// (packed_scene.rs), so the player is built by the scene load, not here.
fn instance(tscn: &str) -> MainLoop {
    let scene = PackedScene::from_tscn(tscn).expect("parse the .tscn");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    add_packed_scene_to_tree(&mut tree, root, &scene).expect("instance the scene");
    MainLoop::new(tree)
}

fn mover_x(ml: &MainLoop) -> f32 {
    let mover = ml
        .tree()
        .get_node_by_path("/root/World/Mover")
        .expect("Mover instanced");
    match ml.tree().get_node(mover).unwrap().get_property("position") {
        Variant::Vector2(v) => v.x,
        other => panic!("Mover.position should be a Vector2, got {other:?}"),
    }
}

/// Asserts the scene load armed the autoplay clip, then that half a second of
/// `MainLoop` frames lands the node on the keyed midpoint and a full second on
/// the last keyframe.
fn assert_clip_drives_the_node(ml: &mut MainLoop, what: &str) {
    let player_node = ml
        .tree()
        .get_node_by_path("/root/World/Anim")
        .expect("AnimationPlayer node instanced");
    let player = ml
        .tree()
        .get_animation_player(player_node)
        .unwrap_or_else(|| {
            panic!(
                "{what}: the scene load attached no AnimationPlayer — the `libraries/` \
                 storage property resolved no library"
            )
        });
    assert!(
        player.animations.contains_key("slide"),
        "{what}: the library's clip must be in the player, got {:?}",
        player.animations.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        player.current_animation(),
        Some("slide"),
        "{what}: autoplay must start the declared clip when the scene enters the tree"
    );

    // The node sits on the first keyframe before any frame runs.
    assert_eq!(mover_x(ml), 0.0, "{what}: starts on the first keyframe");

    // Half of the 1.0s clip, as five real frames.
    for _ in 0..5 {
        ml.step(0.1);
    }
    let mid = mover_x(ml);
    assert!(
        (mid - 50.0).abs() < 1.0,
        "{what}: half-way through the clip the node must sit on the keyed midpoint \
         (~50), got {mid}"
    );

    // Past the end: the node rests on the last keyframe.
    for _ in 0..6 {
        ml.step(0.1);
    }
    let end = mover_x(ml);
    assert!(
        (end - 100.0).abs() < 0.01,
        "{what}: at the end of the clip the node must sit on the last keyframe (100), \
         got {end}"
    );
}

/// **The acceptance test.** Both spellings of a Godot 4.6 animation library
/// load and animate.
///
/// Deliberately ONE test function: the `ExtResource` half configures the
/// process-global `res://` root (`gdcore::file_access::set_path_roots`), and a
/// second `#[test]` in this binary would race it under `cargo test`, which runs
/// a binary's tests as threads in one process.
#[test]
fn animation_player_godot46_library_properties_load() {
    // --- the library as an inline sub-resource -------------------------
    //
    // Needs no `res://` root at all, so it runs first and cannot be blamed on
    // the filesystem wiring below.
    let mut ml = instance(&sub_resource_scene());
    assert_clip_drives_the_node(&mut ml, "libraries/ = SubResource");

    // --- the library as an ExtResource .tres beside the scene ----------
    let dir = unique_project_dir("extlib");
    std::fs::write(dir.join("lib.tres"), library_tres()).expect("write the library .tres");
    // `res://lib.tres` resolves against this root; the scene load reads the
    // file through `FileAccess`, which is also what serves a mounted `.pck`.
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);

    let mut ml = instance(EXT_RESOURCE_SCENE);
    assert_clip_drives_the_node(&mut ml, "libraries/ = ExtResource(.tres)");

    // Leave the global root as it was found rather than pointing at a
    // directory this test is about to delete.
    gdcore::file_access::set_path_roots(None, None);
    cleanup(&dir);
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}
