//! A scene that instances sub-scenes must RUN with them, found importing the
//! official Godot demo "Dodge the Creeps" (godotengine/godot-demo-projects
//! `2d/dodge_the_creeps`): its `main.tscn` instances `player.tscn` and
//! `hud.tscn`, and both booted as childless `Node`s, so the game had no player,
//! no score and no Start button. Every boot path used the plain instancer,
//! which leaves `instance=` nodes as placeholders (the editor's shape); and the
//! one expanding path dropped every sub-scene's own `[connection]`s.
//!
//! Both runtime routes are exercised over real `res://` files:
//! - `add_packed_scene_to_tree_from_res` — the runner, `res_boot`, autoloads;
//! - plain instancing + `expand_instance_placeholders` — editor Play, which
//!   clones the edited (placeholder) tree.

use std::path::PathBuf;
use std::sync::Mutex;

use gdscene::node::Node;
use gdscene::packed_scene::{
    add_packed_scene_to_tree, add_packed_scene_to_tree_from_res, begin_subscene_session,
    expand_instance_placeholders, load_scene_from_res, PackedScene,
};
use gdscene::{NodeId, SceneTree};
use gdvariant::Variant;

/// `set_path_roots` is process-global; tests in this binary take turns.
static ROOTS: Mutex<()> = Mutex::new(());

/// A nested level, so wiring "relative to each level's root" is tested below
/// the first instance too.
const BADGE: &str = r#"[gd_scene format=3]

[node name="Badge" type="Node2D"]

[node name="Timer" type="Timer" parent="."]

[connection signal="timeout" from="Timer" to="." method="_on_timeout"]
"#;

/// The sub-scene, shaped like Dodge the Creeps' `player.tscn`: a typed root
/// with authored values, children, and a connection from itself to itself.
/// It authors no `position`, so its root carries Node2D's SEEDED `(0, 0)` —
/// which must not beat the outer scene's `position` override when the
/// sub-scene is merged into the placeholder.
const PLAYER: &str = r#"[gd_scene format=3]

[ext_resource type="PackedScene" path="res://badge.tscn" id="1_badge"]

[node name="Player" type="Area2D"]
z_index = 10
process_mode = 3

[node name="Sprite" type="Sprite2D" parent="."]

[node name="Inner" parent="." instance=ExtResource("1_badge")]

[connection signal="body_entered" from="." to="." method="_on_body_entered"]
"#;

/// Godot 4.7's own spelling: `unique_id=` on every header, an override entry
/// for a node inside the instance (no `type=`), an addition under the
/// instance, a connection TO the instance, and one FROM a node inside it.
const MAIN: &str = r#"[gd_scene format=3 uid="uid://bmain"]

[ext_resource type="PackedScene" uid="uid://bplayer" path="res://player.tscn" id="1_p"]

[node name="Main" type="Node" unique_id=1]

[node name="Player" parent="." unique_id=2 instance=ExtResource("1_p")]
position = Vector2(5, 6)

[node name="Sprite" parent="Player" unique_id=3]
modulate = Color(1, 0, 0, 1)

[node name="Extra" type="Node2D" parent="Player" unique_id=4]

[connection signal="hit" from="Player" to="." method="game_over"]
[connection signal="timeout" from="Player/Inner/Timer" to="." method="on_inner_timeout"]
"#;

fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "patina-instanced-subscenes-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("badge.tscn"), BADGE).unwrap();
    std::fs::write(dir.join("player.tscn"), PLAYER).unwrap();
    std::fs::write(dir.join("main.tscn"), MAIN).unwrap();
    dir
}

fn node<'a>(tree: &'a SceneTree, from: NodeId, path: &str) -> &'a Node {
    let id = tree
        .get_node_relative(from, path)
        .unwrap_or_else(|| panic!("no node at '{path}'"));
    tree.get_node(id).unwrap()
}

fn id_of(tree: &SceneTree, from: NodeId, path: &str) -> NodeId {
    tree.get_node_relative(from, path)
        .unwrap_or_else(|| panic!("no node at '{path}'"))
}

/// The `(target, method)` pairs connected to `signal` on the node at `path`.
fn connections(tree: &SceneTree, from: NodeId, path: &str, signal: &str) -> Vec<(NodeId, String)> {
    let id = id_of(tree, from, path);
    let Some(store) = tree.signal_store(id) else {
        return Vec::new();
    };
    let Some(sig) = store.get_signal(signal) else {
        return Vec::new();
    };
    sig.connections()
        .iter()
        .map(|c| (NodeId::from_object_id(c.target_id), c.method.clone()))
        .collect()
}

fn children(tree: &SceneTree, id: NodeId) -> Vec<String> {
    tree.get_node(id)
        .unwrap()
        .children()
        .iter()
        .map(|c| tree.get_node(*c).unwrap().name().to_string())
        .collect()
}

/// Everything a running game needs from `MAIN`, whichever route built it.
fn assert_complete(tree: &SceneTree, main: NodeId) {
    let player_id = id_of(tree, main, "Player");
    let player = tree.get_node(player_id).unwrap();
    assert_eq!(player.class_name(), "Area2D", "the instance takes its sub-scene's class");
    assert_eq!(
        children(tree, player_id),
        ["Sprite", "Inner", "Extra"],
        "the sub-scene's children (no duplicate for the override entry), \
         then the outer scene's addition, in Godot's order"
    );

    // Values: the outer override wins, the sub-scene's authored ones survive.
    assert_eq!(
        player.get_property("position"),
        Variant::Vector2(gdcore::math::Vector2::new(5.0, 6.0)),
        "the outer override beats the sub-root's seeded Node2D position default"
    );
    assert_eq!(player.get_property("z_index"), Variant::Int(10));
    assert_eq!(
        player.get_property("process_mode"),
        Variant::Int(3),
        "the sub-scene's own authored values survive the merge"
    );
    let sprite = node(tree, main, "Player/Sprite");
    assert_eq!(sprite.class_name(), "Sprite2D");
    assert_eq!(
        sprite.get_property("modulate"),
        Variant::Color(gdcore::math::Color::new(1.0, 0.0, 0.0, 1.0)),
        "the outer scene's override entry applies to the instanced node"
    );
    assert_eq!(node(tree, main, "Player/Inner").class_name(), "Node2D");
    assert_eq!(node(tree, main, "Player/Inner/Timer").class_name(), "Timer");

    // Godot's ownership: inner nodes belong to the instance that declared them.
    assert_eq!(sprite.owner(), Some(player_id));
    let inner_id = id_of(tree, main, "Player/Inner");
    assert_eq!(node(tree, main, "Player/Inner/Timer").owner(), Some(inner_id));
    assert_eq!(node(tree, main, "Player/Extra").owner(), Some(main));

    // Each level's connections, relative to that level's own root.
    assert_eq!(
        connections(tree, main, "Player", "body_entered"),
        [(player_id, "_on_body_entered".to_string())],
        "the sub-scene's own connection (from . to .) is wired to the instance"
    );
    let timer_conns = connections(tree, main, "Player/Inner/Timer", "timeout");
    assert!(
        timer_conns.contains(&(inner_id, "_on_timeout".to_string())),
        "a nested level's connection is wired relative to ITS root: {timer_conns:?}"
    );
    assert!(
        timer_conns.contains(&(main, "on_inner_timeout".to_string())),
        "the outer scene's connection from a node inside the instance: {timer_conns:?}"
    );
    assert_eq!(
        connections(tree, main, "Player", "hit"),
        [(main, "game_over".to_string())],
        "the outer scene's connection to the instance"
    );
}

#[test]
fn runtime_boot_expands_instanced_subscenes_with_their_connections() {
    let _guard = ROOTS.lock().unwrap_or_else(|p| p.into_inner());
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(project()), None);

    let scene = PackedScene::from_tscn(MAIN).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree_from_res(&mut tree, root, &scene).unwrap();

    assert_complete(&tree, main);
}

#[test]
fn play_expands_the_edited_scenes_placeholders_in_place() {
    let _guard = ROOTS.lock().unwrap_or_else(|p| p.into_inner());
    gdobject::class_db::register_all_classes();
    gdcore::file_access::set_path_roots(Some(project()), None);

    // The editor's shape: the plain instancer leaves `Player` a placeholder.
    let scene = PackedScene::from_tscn(MAIN).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree(&mut tree, root, &scene).unwrap();
    let placeholder = id_of(&tree, main, "Player");
    assert_eq!(
        tree.get_node(placeholder).unwrap().class_name(),
        "Node",
        "precondition: the edited scene holds a placeholder"
    );

    begin_subscene_session();
    let expanded =
        expand_instance_placeholders(&mut tree, main, scene.connections(), &load_scene_from_res);
    assert_eq!(expanded, 1);
    assert_eq!(
        id_of(&tree, main, "Player"),
        placeholder,
        "expanded IN PLACE: the placeholder keeps its id, so what was wired to it survives"
    );
    assert_complete(&tree, main);

    // A second pass changes nothing (no duplicated children or connections).
    assert_eq!(
        expand_instance_placeholders(&mut tree, main, scene.connections(), &load_scene_from_res),
        0
    );
    assert_complete(&tree, main);
}

#[test]
fn an_unloadable_subscene_stays_a_placeholder_and_the_rest_still_boots() {
    let _guard = ROOTS.lock().unwrap_or_else(|p| p.into_inner());
    gdobject::class_db::register_all_classes();
    let dir = project();
    gdcore::file_access::set_path_roots(Some(dir), None);

    let broken = MAIN.replace("res://player.tscn", "res://missing.tscn");
    let scene = PackedScene::from_tscn(&broken).unwrap();
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    let main = add_packed_scene_to_tree_from_res(&mut tree, root, &scene).unwrap();
    let player = node(&tree, main, "Player");
    assert_eq!(player.class_name(), "Node", "no sub-scene to take a class from");
    assert_eq!(
        player.get_property("position"),
        Variant::Vector2(gdcore::math::Vector2::new(5.0, 6.0)),
        "the outer scene's values still apply"
    );
}
