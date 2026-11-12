//! pat-m2jgd: a scene that instances an imported `.glb` with EDITABLE CHILDREN
//! must instance — found importing the official Godot 3D platformer demo
//! (godotengine/godot-demo-projects `3d/platformer`, `games/platformer_3d`).
//!
//! `player.tscn` instances `player.glb` and then edits nodes INSIDE it:
//!
//! ```text
//! [node name="Player" parent="." unique_id=840561040 instance=ExtResource("3_uccbe")]
//! [node name="Robot" parent="Player/Skeleton/Skeleton3D"
//!       parent_id_path=PackedInt32Array(840561040, 1535497167) index="0" ...]
//! [node name="Bullet" type="Marker3D" parent="Player/Skeleton" ...]
//! ```
//!
//! Patina read a `.glb` sub-scene as `.tscn` text, failed, left the instance an
//! empty `Node`, and then every editable entry under it failed the WHOLE scene
//! ("parent path 'Player/Skeleton/Skeleton3D' not found for node 'Robot'"). So
//! `game.tscn` had no player, no Camera3D (it lives in player.tscn) and no
//! enemies. Even a correctly-read glb had no `Skeleton3D`: every joint was a
//! plain `Node3D`, which is not what Godot's importer builds.
//!
//! Every test here loads the REAL, unmodified upstream files.

use std::path::PathBuf;
use std::sync::Mutex;

use gdscene::node::Node;
use gdscene::packed_scene::{
    add_packed_scene_to_tree_from_res, begin_subscene_session, load_scene_from_res,
};
use gdscene::{skeleton3d, NodeId, SceneTree};
use gdvariant::Variant;

/// `set_path_roots` is process-global; tests in this binary take turns.
static ROOTS: Mutex<()> = Mutex::new(());

fn platformer_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../games/platformer_3d")
}

/// Instances `res://<scene>` from the platformer project the way the runner
/// boots a scene: every `instance=` expanded.
fn boot(scene: &str) -> (SceneTree, NodeId) {
    let root = platformer_root();
    assert!(
        root.join("project.godot").exists(),
        "fixture missing: {}",
        root.display()
    );
    gdcore::file_access::set_path_roots(Some(root), None);
    begin_subscene_session();
    let packed = load_scene_from_res(&format!("res://{scene}"))
        .unwrap_or_else(|| panic!("res://{scene} must load"));
    let mut tree = SceneTree::new();
    let tree_root = tree.root_id();
    let id = add_packed_scene_to_tree_from_res(&mut tree, tree_root, &packed)
        .unwrap_or_else(|e| panic!("res://{scene} must instance: {e}"));
    (tree, id)
}

fn node<'a>(tree: &'a SceneTree, from: NodeId, path: &str) -> &'a Node {
    let id = id_of(tree, from, path);
    tree.get_node(id).unwrap()
}

fn id_of(tree: &SceneTree, from: NodeId, path: &str) -> NodeId {
    tree.get_node_relative(from, path)
        .unwrap_or_else(|| panic!("no node at '{path}'"))
}

fn child_names(tree: &SceneTree, id: NodeId) -> Vec<String> {
    tree.get_node(id)
        .unwrap()
        .children()
        .iter()
        .map(|c| tree.get_node(*c).unwrap().name().to_string())
        .collect()
}

#[test]
fn player_tscn_instances_the_glb_hierarchy_with_its_editable_children() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, player) = boot("player/player.tscn");

    assert_eq!(node(&tree, player, ".").class_name(), "CharacterBody3D");
    // The glb instance: Godot's imported-scene shape, Skeleton3D generated
    // under the node that parents the skin's root joint.
    assert_eq!(node(&tree, player, "Player").class_name(), "Node3D");
    assert_eq!(
        node(&tree, player, "Player/Skeleton").class_name(),
        "Node3D"
    );
    let skeleton = id_of(&tree, player, "Player/Skeleton/Skeleton3D");
    assert_eq!(tree.get_node(skeleton).unwrap().class_name(), "Skeleton3D");
    assert_eq!(
        child_names(&tree, id_of(&tree, player, "Player")),
        ["Skeleton", "AnimationPlayer"],
        "the glb root holds its scene root node and the AnimationPlayer"
    );

    // The joints are BONES, not nodes: 27 joints in player.glb's one skin.
    assert_eq!(skeleton3d::get_bone_count(&tree, skeleton), 27);
    let master = skeleton3d::find_bone(&tree, skeleton, "MASTER").expect("MASTER bone");
    assert_eq!(master, 0, "the skin's root joint is bone 0");
    assert_eq!(skeleton3d::get_bone_parent(&tree, skeleton, master), -1);
    let hip = skeleton3d::find_bone(&tree, skeleton, "hip").expect("hip bone");
    assert_eq!(
        skeleton3d::get_bone_parent(&tree, skeleton, hip),
        master as i64
    );
    assert!(
        tree.get_node_relative(player, "Player/Skeleton/MASTER")
            .is_none(),
        "a joint must not ALSO exist as a Node3D"
    );

    // The skinned mesh was reparented under the skeleton, and the editable
    // override (`layers = 2`) landed on it — not on a duplicate.
    let robot = node(&tree, player, "Player/Skeleton/Skeleton3D/Robot");
    assert_eq!(robot.class_name(), "MeshInstance3D");
    assert_eq!(robot.get_property("layers"), Variant::Int(2));
    assert_eq!(robot.get_property("__skin__bind_count"), Variant::Int(27));
    assert_eq!(
        child_names(&tree, skeleton),
        ["Robot"],
        "exactly one Robot: the override modified the instanced mesh"
    );

    // New nodes the outer scene added under the instance, at the indices it
    // wrote (`index="1"`, `index="2"`), with their own children.
    assert_eq!(
        child_names(&tree, id_of(&tree, player, "Player/Skeleton")),
        ["Skeleton3D", "Bullet", "CoinCount"]
    );
    assert_eq!(
        node(&tree, player, "Player/Skeleton/Bullet").class_name(),
        "Marker3D"
    );
    assert_eq!(
        child_names(&tree, id_of(&tree, player, "Player/Skeleton/CoinCount")),
        ["Parallax", "Parallax2", "Parallax3", "Parallax4"]
    );

    // The camera lives in player.tscn; it must exist once the scene instances.
    assert_eq!(
        node(&tree, player, "Target/Camera3D").class_name(),
        "Camera3D"
    );
}

#[test]
fn player_glb_clips_key_bones_and_reach_the_animation_tree() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, player) = boot("player/player.tscn");

    let ap_id = id_of(&tree, player, "Player/AnimationPlayer");
    let ap = tree
        .get_animation_player(ap_id)
        .expect("the glb's AnimationPlayer carries its clips");
    for clip in [
        "idle",
        "walk",
        "run",
        "jump",
        "falling",
        "shooting_standing",
    ] {
        assert!(ap.animations.contains_key(clip), "missing clip '{clip}'");
    }
    // A joint channel keys `<skeleton path>:<bone>` relative to the glb root,
    // exactly what the AnimationTree's `filters` name
    // ("Skeleton/Skeleton3D:hip").
    let run = &ap.animations["run"];
    assert!(
        run.tracks
            .iter()
            .any(|t| t.node_path == "Skeleton/Skeleton3D" && t.property_path == "hip"),
        "the run clip must drive the hip BONE; tracks were {:?}",
        run.tracks
            .iter()
            .take(4)
            .map(|t| (t.node_path.clone(), t.property_path.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        ap.root_node, "..",
        "Godot's AnimationMixer.root_node default"
    );

    // The AnimationTree (`anim_player = NodePath("../Player/AnimationPlayer")`)
    // resolves its clips from that player.
    let at_id = id_of(&tree, player, "AnimationTree");
    let at = tree
        .get_animation_tree(at_id)
        .expect("player.tscn's AnimationTree attaches");
    assert!(
        at.animations.contains_key("run") && at.animations.contains_key("idle"),
        "the AnimationTree must see the glb's clips, saw {:?}",
        at.animations.keys().collect::<Vec<_>>()
    );
}

#[test]
fn enemy_tscn_edits_the_glb_skeleton_and_its_animation_player() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, enemy) = boot("enemy/enemy.tscn");

    assert_eq!(node(&tree, enemy, ".").class_name(), "RigidBody3D");
    let skeleton = id_of(&tree, enemy, "Enemy/Skeleton/Skeleton3D");
    assert_eq!(tree.get_node(skeleton).unwrap().class_name(), "Skeleton3D");
    assert_eq!(
        child_names(&tree, id_of(&tree, enemy, "Enemy/Skeleton")),
        ["Skeleton3D", "RayFloor", "RayWall"]
    );

    // Bone ORDER matches Godot's: enemy.tscn addresses `bones/1` (body) and
    // `bones/2` (eyes, body's child) by index, so a breadth-first order would
    // apply eyes' override to a leg.
    assert_eq!(
        skeleton3d::get_bone_name(&tree, skeleton, 0).as_deref(),
        Some("MASTER")
    );
    assert_eq!(
        skeleton3d::get_bone_name(&tree, skeleton, 1).as_deref(),
        Some("body")
    );
    assert_eq!(
        skeleton3d::get_bone_name(&tree, skeleton, 2).as_deref(),
        Some("eyes")
    );
    assert_eq!(
        skeleton3d::get_bone_name(&tree, skeleton, 3).as_deref(),
        Some("r-frontleg")
    );
    // ...and the editable override's pose reached bone 2 (eyes).
    let eyes = skeleton3d::get_bone_pose_position(&tree, skeleton, 2);
    assert!(
        (eyes.y - 0.475726).abs() < 1e-5 && (eyes.z + 0.0617121).abs() < 1e-5,
        "bones/2/position override must apply to eyes, got {eyes:?}"
    );

    // The AnimationPlayer override re-libraries the glb's player with
    // enemy.tscn's OWN sub-resources (including the `extra` library) and
    // autoplays `walk`.
    let ap_id = id_of(&tree, enemy, "Enemy/AnimationPlayer");
    let ap = tree
        .get_animation_player(ap_id)
        .expect("enemy player attached");
    assert!(
        ap.animations.keys().any(|k| k.ends_with("explode")),
        "the `extra` library from enemy.tscn must be present, saw {:?}",
        ap.animations.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        ap.current_animation(),
        Some("walk"),
        "autoplay = &\"walk\" (autoplay {:?}, clips {:?}, node prop {:?})",
        ap.autoplay,
        ap.animations.keys().collect::<Vec<_>>(),
        node(&tree, enemy, "Enemy/AnimationPlayer").get_property("autoplay")
    );
}

/// `parent_id_path` addresses the parent by `unique_id`s, which survive a
/// rename inside the instanced scene; the NodePath beside it is then stale.
/// Godot resolves the id path first.
#[test]
fn parent_id_path_finds_a_parent_whose_path_went_stale() {
    use gdscene::packed_scene::PackedScene;
    const ROBOT: &str = r#"[gd_scene format=3]

[node name="Robot" type="Node3D" unique_id=1]

[node name="Arm" type="Node3D" parent="." unique_id=7]
"#;
    // Saved when the arm was still called `OldArm`.
    const OUTER: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://robot.tscn" id="1_r"]

[node name="Main" type="Node3D" unique_id=100]

[node name="Bot" parent="." unique_id=2 instance=ExtResource("1_r")]

[node name="Hand" type="Marker3D" parent="Bot/OldArm" parent_id_path=PackedInt32Array(2, 7) unique_id=3]

[editable path="Bot"]
"#;
    let outer = PackedScene::from_tscn(OUTER).unwrap();
    let resolve =
        |path: &str| (path == "res://robot.tscn").then(|| PackedScene::from_tscn(ROBOT).unwrap());
    let nodes = outer
        .instance_with_subscenes(&resolve)
        .expect("the id path resolves even though `Bot/OldArm` no longer exists");
    let arm = nodes.iter().find(|n| n.name() == "Arm").expect("Arm");
    let hand = nodes.iter().find(|n| n.name() == "Hand").expect("Hand");
    assert_eq!(hand.class_name(), "Marker3D");
    assert_eq!(
        hand.parent(),
        Some(arm.id()),
        "Hand must land under the renamed Arm"
    );
}

/// Godot quotes a property name with spaces: player.tscn's blend tree writes
/// `"nodes/Animation 2/node" = SubResource("13")`. The quotes are syntax, not
/// part of the name.
#[test]
fn a_quoted_property_key_is_decoded() {
    let src = std::fs::read_to_string(platformer_root().join("player/player.tscn")).unwrap();
    let scene = gdscene::packed_scene::PackedScene::from_tscn(&src).unwrap();
    let blend_tree = scene
        .sub_resource_props("23")
        .expect("blend tree sub-resource");
    assert_eq!(
        blend_tree.get("nodes/Animation 2/node"),
        Some(&Variant::String("SubResource:13".into()))
    );
    assert!(
        !blend_tree.keys().any(|k| k.starts_with('"')),
        "no key may keep its quotes: {:?}",
        blend_tree
            .keys()
            .filter(|k| k.starts_with('"'))
            .collect::<Vec<_>>()
    );
}

#[test]
fn game_tscn_has_a_real_player_with_its_camera_and_real_enemies() {
    let _g = ROOTS.lock().unwrap_or_else(|e| e.into_inner());
    let (tree, game) = boot("game.tscn");

    assert_eq!(node(&tree, game, "Player").class_name(), "CharacterBody3D");
    assert_eq!(
        node(&tree, game, "Player/Target/Camera3D").class_name(),
        "Camera3D"
    );
    assert_eq!(
        node(&tree, game, "Player/Player/Skeleton/Skeleton3D").class_name(),
        "Skeleton3D"
    );
    let enemies = id_of(&tree, game, "Enemies");
    let names = child_names(&tree, enemies);
    assert!(!names.is_empty(), "game.tscn places enemies");
    for name in names {
        let path = format!("Enemies/{name}");
        assert_eq!(
            node(&tree, game, &path).class_name(),
            "RigidBody3D",
            "{path}"
        );
        assert_eq!(
            node(&tree, game, &format!("{path}/Enemy/Skeleton/Skeleton3D")).class_name(),
            "Skeleton3D",
            "{path}"
        );
    }
}
