//! pat-0q2kx.139 — instance-internal overrides, inherited scenes, and
//! `InstancePlaceholder`.
//!
//! Godot ground truth (`scene/resources/packed_scene.cpp` SceneState):
//! - a later `[node]` whose parent path points *inside* an instanced sub-scene,
//!   with no `type=`, OVERRIDES that existing node — this is how every tweaked
//!   instance is saved;
//! - a scene whose root `[node]` carries `instance=` and no `type=` is an
//!   INHERITED scene: the base instantiates first, the diff applies on top;
//! - `instance_placeholder=` yields an `InstancePlaceholder` exposing
//!   `get_stored_values()` / `get_instance_path()` / `create_instance()`.
//!
//! Patina did none of the three. Override entries hit the regular branch, which
//! unconditionally built a new node — producing a DUPLICATE sibling while the
//! real instanced child kept its base values. The root template's `instance=`
//! was skipped outright, so an inherited scene lost its whole base tree. And
//! placeholders loaded as plain empty `Node`s.

use gdcore::math::Vector2;
use gdscene::node::NodeId;
use gdscene::packed_scene::{
    add_packed_scene_to_tree_with_subscenes, placeholder_create_instance,
    placeholder_instance_path, placeholder_stored_values, PackedScene,
};
use gdscene::scene_tree::SceneTree;
use gdvariant::Variant;

/// The sub-scene both the instancing and the inheritance cases build on.
const ENEMY_TSCN: &str = r#"[gd_scene format=3]

[node name="Enemy" type="Node2D"]
position = Vector2(1, 1)

[node name="Sprite" type="Sprite2D" parent="."]
position = Vector2(5, 5)

[node name="Hitbox" type="Area2D" parent="Sprite"]
"#;

fn resolver(path: &str) -> Option<PackedScene> {
    match path {
        "res://enemy.tscn" => PackedScene::from_tscn(ENEMY_TSCN).ok(),
        _ => None,
    }
}

/// Loads `src` into a fresh tree and returns `(tree, scene_root)`.
fn load(src: &str) -> (SceneTree, NodeId) {
    let scene = PackedScene::from_tscn(src).expect("scene parses");
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let scene_root = add_packed_scene_to_tree_with_subscenes(&mut tree, root, &scene, &resolver)
        .expect("scene instances");
    (tree, scene_root)
}

/// Every descendant of `parent` named `name`.
fn children_named(tree: &SceneTree, parent: NodeId, name: &str) -> Vec<NodeId> {
    tree.get_node(parent)
        .map(|n| n.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|id| {
            tree.get_node(*id)
                .map(|n| n.name() == name)
                .unwrap_or(false)
        })
        .collect()
}

fn position_of(tree: &SceneTree, id: NodeId) -> Vector2 {
    match tree.get_node(id).map(|n| n.get_property("position")) {
        Some(Variant::Vector2(v)) => v,
        other => panic!("expected a Vector2 position, got {other:?}"),
    }
}

/// **The acceptance test.** An override of an instanced child modifies that
/// child rather than adding a duplicate sibling.
#[test]
fn packed_scene_instance_overrides() {
    const MAIN: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="1_enemy"]

[node name="Main" type="Node2D"]

[node name="Enemy" parent="." instance=ExtResource("1_enemy")]

[node name="Sprite" parent="Enemy"]
position = Vector2(9, 9)
"#;

    let (tree, main) = load(MAIN);

    let enemies = children_named(&tree, main, "Enemy");
    assert_eq!(enemies.len(), 1, "one instanced Enemy");
    let enemy = enemies[0];

    // The heart of it: ONE Sprite, carrying the override.
    let sprites = children_named(&tree, enemy, "Sprite");
    assert_eq!(
        sprites.len(),
        1,
        "the override must modify the instanced Sprite, not add a duplicate \
         sibling — found {} Sprites under Enemy",
        sprites.len(),
    );
    assert_eq!(
        position_of(&tree, sprites[0]),
        Vector2::new(9.0, 9.0),
        "the overridden position must win over the sub-scene's own",
    );

    // The sub-scene's deeper structure survives the override.
    assert_eq!(
        children_named(&tree, sprites[0], "Hitbox").len(),
        1,
        "overriding a node must not drop its children",
    );
}

/// An inherited scene loads the base tree and applies the diff on top.
#[test]
fn inherited_scene_loads_base_tree_with_diff() {
    const INHERITED: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="1_enemy"]

[node name="Boss" instance=ExtResource("1_enemy")]
position = Vector2(7, 7)

[node name="Crown" type="Sprite2D" parent="."]

[node name="Sprite" parent="."]
position = Vector2(3, 3)
"#;

    let (tree, boss) = load(INHERITED);

    // The base tree came along — this is what used to vanish entirely.
    let sprites = children_named(&tree, boss, "Sprite");
    assert_eq!(
        sprites.len(),
        1,
        "the inherited base's Sprite must exist exactly once",
    );
    assert_eq!(
        children_named(&tree, sprites[0], "Hitbox").len(),
        1,
        "the base's grandchild must come along too",
    );

    // The root diff applied on top of the base root's own `position`.
    assert_eq!(position_of(&tree, boss), Vector2::new(7.0, 7.0));

    // A diff entry targeting a base child overrides it rather than duplicating.
    assert_eq!(position_of(&tree, sprites[0]), Vector2::new(3.0, 3.0));

    // And the derived scene's own extra child is added.
    assert_eq!(
        children_named(&tree, boss, "Crown").len(),
        1,
        "the derived scene's new child must be added to the base tree",
    );
}

/// `instance_placeholder` loads as an `InstancePlaceholder` that can swap in the
/// real sub-scene on demand.
#[test]
fn instance_placeholder_defers_then_creates_the_instance() {
    const WITH_PLACEHOLDER: &str = r#"[gd_scene load_steps=2 format=3]

[ext_resource type="PackedScene" path="res://enemy.tscn" id="1_enemy"]

[node name="Main" type="Node2D"]

[node name="Deferred" parent="." instance_placeholder="res://enemy.tscn"]
position = Vector2(4, 4)
"#;

    let (mut tree, main) = load(WITH_PLACEHOLDER);

    let deferred = children_named(&tree, main, "Deferred");
    assert_eq!(deferred.len(), 1);
    let placeholder = deferred[0];

    assert_eq!(
        tree.get_node(placeholder)
            .map(|n| n.class_name().to_string()),
        Some("InstancePlaceholder".to_string()),
        "a placeholder must load as InstancePlaceholder, not a plain Node",
    );
    assert_eq!(
        placeholder_instance_path(&tree, placeholder).as_deref(),
        Some("res://enemy.tscn"),
    );
    // The authored properties are *stored*, not applied to the stand-in.
    assert_eq!(
        placeholder_stored_values(&tree, placeholder),
        vec![(
            "position".to_string(),
            Variant::Vector2(Vector2::new(4.0, 4.0))
        )],
    );
    // Nothing was instanced yet — that is the whole point of deferring.
    assert!(
        children_named(&tree, placeholder, "Sprite").is_empty(),
        "the sub-scene must not be loaded until create_instance()",
    );

    // Swap in the real scene.
    let new_root =
        placeholder_create_instance(&mut tree, placeholder, &resolver).expect("instance created");

    assert_eq!(
        children_named(&tree, new_root, "Sprite").len(),
        1,
        "create_instance() must bring in the real sub-scene's children",
    );
    assert_eq!(
        position_of(&tree, new_root),
        Vector2::new(4.0, 4.0),
        "the placeholder's stored values must be applied to the new instance",
    );
    // The placeholder itself is gone, not left alongside the instance.
    assert!(
        tree.get_node(placeholder).is_none(),
        "the superseded placeholder must be removed",
    );
}
