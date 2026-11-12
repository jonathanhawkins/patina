//! `play()` with no name keeps the CURRENT animation (Godot:
//! `play(StringName name = &"")`). A Godot 4 `.tscn` stores the current
//! animation as a StringName (`animation = &"right"`); Patina's `play()` only
//! recognised a String, swapped in `"default"` (a clip the scene lacks), and
//! the sprite froze on frame 0. Found replaying Dodge the Creeps in the browser,
//! so the test drives that game's own player scene.

use gdscene::{GDScriptNodeInstance, SceneTree};
use gdvariant::Variant;

fn anim_name(v: &Variant) -> String {
    match v {
        Variant::String(s) => s.clone(),
        Variant::StringName(s) => s.as_str().to_string(),
        other => panic!("animation: {other:?}"),
    }
}

#[test]
fn play_without_a_name_advances_the_scene_authored_stringname_animation() {
    gdobject::class_db::register_all_classes();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/dodge_the_creeps");
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let tscn =
        std::fs::read_to_string(dir.join("player.tscn")).expect("Dodge the Creeps player.tscn");
    assert!(
        tscn.contains("animation = &\"right\""),
        "fixture authors a StringName animation"
    );
    let packed = gdscene::packed_scene::PackedScene::from_tscn(&tscn).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree_from_res(&mut tree, root, &packed).unwrap();
    let spr = tree
        .get_node_by_path("/root/Player/AnimatedSprite2D")
        .expect("sprite");
    let src = "extends AnimatedSprite2D\nfunc _ready():\n\tplay()\n";
    tree.attach_script(
        spr,
        Box::new(GDScriptNodeInstance::from_source(src, spr).unwrap()),
    );
    gdscene::LifecycleManager::enter_tree(&mut tree, spr);

    assert_eq!(
        anim_name(&tree.get_node(spr).unwrap().get_property("animation")),
        "right",
        "play() must keep the current animation, not switch to \"default\""
    );
    // "right" is 2 frames at 5 fps: 0.25 s lands on frame 1.
    let mut ml = gdscene::main_loop::MainLoop::new(tree);
    for _ in 0..15 {
        ml.step(1.0 / 60.0);
    }
    assert_eq!(
        ml.tree().get_node(spr).unwrap().get_property("frame"),
        Variant::Int(1),
        "the clip must advance"
    );
}

/// Calling `play()` every frame on the clip that is already running keeps the
/// playhead (Godot only rewinds on an animation CHANGE) — the `if moving:
/// play()` per-`_process` pattern Dodge the Creeps' player uses.
#[test]
fn play_every_frame_on_the_running_clip_keeps_advancing() {
    gdobject::class_db::register_all_classes();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../games/dodge_the_creeps");
    gdcore::file_access::set_path_roots(Some(dir.clone()), None);
    let packed = gdscene::packed_scene::PackedScene::from_tscn(
        &std::fs::read_to_string(dir.join("player.tscn")).unwrap(),
    )
    .unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    gdscene::packed_scene::add_packed_scene_to_tree_from_res(&mut tree, root, &packed).unwrap();
    let spr = tree
        .get_node_by_path("/root/Player/AnimatedSprite2D")
        .unwrap();
    let src = "extends AnimatedSprite2D\nfunc _process(_d):\n\tplay()\n\tanimation = &\"right\"\n";
    tree.attach_script(
        spr,
        Box::new(GDScriptNodeInstance::from_source(src, spr).unwrap()),
    );
    gdscene::LifecycleManager::enter_tree(&mut tree, spr);
    let mut ml = gdscene::main_loop::MainLoop::new(tree);
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..30 {
        ml.step(1.0 / 60.0);
        if let Variant::Int(f) = ml.tree().get_node(spr).unwrap().get_property("frame") {
            seen.insert(f);
        }
    }
    assert!(
        seen.contains(&1),
        "re-calling play() each frame must not pin frame 0: saw {seen:?}"
    );
}
