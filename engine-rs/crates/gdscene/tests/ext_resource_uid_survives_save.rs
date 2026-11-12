//! pat-0q2kx.137 (partial) — an `[ext_resource]`'s `uid=` survives an
//! open→save round-trip.
//!
//! ⚠️ This does NOT satisfy the bead's acceptance (`tscn_save_lossless_roundtrip`),
//! which additionally needs the shared variant writer and an editor `.tres`
//! save route. See the completion report; the bead's connection-flags claim is
//! already fixed and was stale.
//!
//! Godot writes `uid=` on every `[ext_resource]` it saves
//! (scene/resources/resource_format_text.cpp) and resolves a moved file through
//! it — the path is the fallback, not the identity.
//!
//! Patina's loader parsed the attribute and threw it away: `ExtResourceEntry`
//! held only `res_type` and `path`. So an editor open+save rewrote every
//! reference without its uid, silently degrading the scene to path-only
//! references that stop resolving as soon as a file moves. The header still
//! looked plausible in a diff, which is how it survived.

use gdscene::node::NodeId;
use gdscene::packed_scene::{add_packed_scene_to_tree, PackedScene};
use gdscene::scene_saver::TscnSaver;
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// A scene whose script, instanced sub-scene and texture references all carry
/// a uid — the three shapes the saver rebuilds its `[ext_resource]` table from.
const SCENE: &str = r#"[gd_scene load_steps=4 format=3 uid="uid://scene_root"]

[ext_resource type="Script" uid="uid://the_script" path="res://player.gd" id="1_script"]
[ext_resource type="Texture2D" uid="uid://the_icon" path="res://icon.png" id="2_tex"]
[ext_resource type="Texture2D" path="res://no_uid.png" id="3_plain"]

[node name="Root" type="Node2D"]
script = ExtResource("1_script")

[node name="Icon" type="Sprite2D" parent="."]
texture = ExtResource("2_tex")

[node name="Plain" type="Sprite2D" parent="."]
texture = ExtResource("3_plain")
"#;

fn load(tscn: &str) -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(tscn).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree(&mut tree, root, &scene).expect("scene instances");
    (tree, scene_root)
}

/// The `[ext_resource]` line for `path`, from saved `.tscn` text.
fn ext_line<'a>(saved: &'a str, path: &str) -> &'a str {
    saved
        .lines()
        .find(|l| l.starts_with("[ext_resource") && l.contains(&format!("path=\"{path}\"")))
        .unwrap_or_else(|| panic!("no [ext_resource] row for {path} in:\n{saved}"))
}

#[test]
fn ext_resource_uid_survives_save_roundtrip() {
    let (tree, scene_root) = load(SCENE);
    let saved = TscnSaver::save_tree(&tree, scene_root);

    // Each reference keeps the uid it was authored with.
    assert!(
        ext_line(&saved, "res://player.gd").contains("uid=\"uid://the_script\""),
        "the script ext_resource must keep its uid:\n{saved}"
    );
    assert!(
        ext_line(&saved, "res://icon.png").contains("uid=\"uid://the_icon\""),
        "the texture ext_resource must keep its uid:\n{saved}"
    );

    // A resource that never had a uid must NOT gain an empty one — Godot omits
    // the attribute entirely rather than writing uid="".
    let plain = ext_line(&saved, "res://no_uid.png");
    assert!(
        !plain.contains("uid="),
        "a resource with no uid must not gain one: {plain}"
    );

    // The saved text must still parse, with the references intact.
    let (tree2, root2) = load(&saved);
    let icon = tree2
        .get_node_or_null(root2, "Icon")
        .expect("Icon survives the round-trip");
    assert_eq!(
        tree2
            .get_node(icon)
            .unwrap()
            .get_property("__extres__texture"),
        Variant::String("res://icon.png".into()),
        "the reloaded texture must still resolve to its path"
    );

    // ...and a second save is stable, so the uid is not lost on the next pass.
    let saved2 = TscnSaver::save_tree(&tree2, root2);
    assert!(
        ext_line(&saved2, "res://icon.png").contains("uid=\"uid://the_icon\""),
        "the uid must survive repeated open+save cycles:\n{saved2}"
    );
}

/// The uid cache is bookkeeping: it must never be written out as a node
/// property, which would invent a property Godot cannot parse.
#[test]
fn the_uid_companion_is_not_emitted_as_a_property() {
    let (tree, scene_root) = load(SCENE);
    let saved = TscnSaver::save_tree(&tree, scene_root);
    assert!(
        !saved.contains("__extuid__"),
        "the __extuid__ companion must stay internal:\n{saved}"
    );
    assert!(
        !saved.contains("__extres__"),
        "the __extres__ companion must stay internal too:\n{saved}"
    );
}

/// A scene with no uids anywhere still saves and reloads unchanged — the new
/// attribute is strictly additive.
#[test]
fn a_scene_without_uids_is_unaffected() {
    const PLAIN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://icon.png" id="1_tex"]

[node name="Root" type="Node2D"]

[node name="Icon" type="Sprite2D" parent="."]
texture = ExtResource("1_tex")
"#;
    let (tree, scene_root) = load(PLAIN);
    let saved = TscnSaver::save_tree(&tree, scene_root);
    assert!(
        !saved.contains("uid="),
        "a scene with no uids must not grow any:\n{saved}"
    );
    // Still reloads with the reference intact.
    let (tree2, root2) = load(&saved);
    let icon = tree2.get_node_or_null(root2, "Icon").expect("Icon reloads");
    assert_eq!(
        tree2
            .get_node(icon)
            .unwrap()
            .get_property("__extres__texture"),
        Variant::String("res://icon.png".into())
    );
}
